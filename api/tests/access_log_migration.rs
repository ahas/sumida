//! `entity::migration::access_log` against a sign-in log left in the pre-migration shape.
//!
//! An installation whose `access_logs` predates the primary key holds the shape the table's own
//! creation made for it: a serial-style `id` — `DEFAULT nextval(...)` with a sequence the column
//! owns — and no primary key constraint. The renumbering step of that migration runs exactly where
//! that shape stands, and a generator still bound to the column makes `ADD GENERATED … AS IDENTITY`
//! refuse the whole step.
//!
//! The repair cannot be reached through `Migrator::up`, because a migrated database passes the step
//! early — the primary key is already there. So this test rebuilds the legacy shape over the fresh
//! test database, invokes the migration's steps directly, and checks what the column and the table
//! are left as, in and out of the round trip, and once more with the default drawing on a sequence
//! another table owns.

use e2e::migration::{MigrationTrait, SchemaManager};
use e2e::prelude::*;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};

/// The columns this migration reads, in the shape `integer().auto_increment()` leaves on Postgres
/// and no primary key: an explicit `DEFAULT nextval` over a sequence `OWNED BY` the column — what
/// the serial sugar expands to — and a duplicated `id` whose rows sit out of written order.
const LEGACY_ACCESS_LOGS: &str = r#"
  DROP TABLE IF EXISTS access_logs CASCADE;
  CREATE TABLE access_logs (
    id integer NOT NULL,
    user_id integer,
    method text NOT NULL,
    code integer NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
  );
  CREATE SEQUENCE access_logs_id_seq;
  ALTER TABLE access_logs ALTER COLUMN id SET DEFAULT nextval('access_logs_id_seq'::regclass);
  ALTER SEQUENCE access_logs_id_seq OWNED BY access_logs.id;
  INSERT INTO access_logs (id, method, code, created_at) VALUES
    (7, 'password', 401, '2026-01-03T00:00:00Z'),
    (2, 'password', 200, '2026-01-01T00:00:00Z'),
    (7, 'token',    403, '2026-01-02T00:00:00Z');
"#;

/// The same missing primary key and duplicated ids, but the default draws on `user_logs_id_seq` —
/// a sequence a separate table owns and would keep using — as it does in an installation that
/// archived its sign-in log under an older name.
const SHARED_SEQUENCE_ACCESS_LOGS: &str = r#"
  DROP TABLE IF EXISTS access_logs CASCADE;
  DROP TABLE IF EXISTS legacy_carrier CASCADE;
  CREATE TABLE legacy_carrier (id integer, note text);
  CREATE SEQUENCE user_logs_id_seq;
  ALTER TABLE legacy_carrier ALTER COLUMN id SET DEFAULT nextval('user_logs_id_seq'::regclass);
  ALTER SEQUENCE user_logs_id_seq OWNED BY legacy_carrier.id;
  CREATE TABLE access_logs (
    id integer NOT NULL DEFAULT nextval('user_logs_id_seq'::regclass),
    user_id integer,
    method text NOT NULL,
    code integer NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
  );
  INSERT INTO access_logs (id, method, code, created_at) VALUES
    (5, 'token',    403, '2026-01-03T00:00:00Z'),
    (5, 'password', 200, '2026-01-01T00:00:00Z'),
    (2, 'password', 401, '2026-01-02T00:00:00Z');
"#;

async fn scalar(db: &DatabaseConnection, sql: &str) -> Option<QueryResult> {
  db.query_one(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
    .await
    .unwrap_or_else(|e| panic!("catalog query failed: {sql}\n{e}"))
}

/// What the `id` column and the table are at this moment, as the catalog reports them.
async fn shape(db: &DatabaseConnection) -> (String, Option<String>, bool, i64) {
  let row = scalar(
    db,
    r#"
    SELECT a.attidentity::text AS identity,
           pg_get_expr(d.adbin, d.adrelid) AS default_expr,
           EXISTS (
             SELECT 1 FROM pg_constraint c
             WHERE c.conrelid = 'access_logs'::regclass AND c.contype = 'p'
           ) AS has_pk,
           (
             SELECT count(*) FROM pg_depend dep
             JOIN pg_class s ON s.oid = dep.objid AND s.relkind = 'S'
             WHERE dep.classid = 'pg_class'::regclass
               AND dep.objsubid = 0
               AND dep.deptype = 'a'
               AND dep.refobjid = 'access_logs'::regclass
               AND dep.refobjsubid = a.attnum
           ) AS owned_sequences
    FROM pg_attribute a
    LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    WHERE a.attrelid = 'access_logs'::regclass AND a.attname = 'id'
    "#,
  )
  .await
  .expect("the id column is there");
  (
    row.try_get::<String>("", "identity").unwrap(),
    row.try_get::<Option<String>>("", "default_expr").unwrap(),
    row.try_get::<bool>("", "has_pk").unwrap(),
    row.try_get::<i64>("", "owned_sequences").unwrap(),
  )
}

/// Every `id` distinct and dense from one, matching the order the rows were written in.
async fn renumbered_oldest_first(db: &DatabaseConnection) -> bool {
  let row = scalar(
    db,
    r#"
    SELECT bool_and(ord) AS ordered,
           count(*) = count(DISTINCT id) AS distinct_ids,
           min(id) = 1 AND max(id) = count(*) AS dense
    FROM (
      SELECT id, row_number() OVER (ORDER BY created_at, ctid) = id AS ord
      FROM access_logs
    ) s
    "#,
  )
  .await
  .expect("the count row is there");
  row.try_get::<bool>("", "ordered").unwrap()
    && row.try_get::<bool>("", "distinct_ids").unwrap()
    && row.try_get::<bool>("", "dense").unwrap()
}

/// The next written row takes its own number from the generator, past the last row.
async fn next_generated_id(db: &DatabaseConnection) -> i32 {
  scalar(
    db,
    "INSERT INTO access_logs (user_id, method, code) VALUES (NULL, 'token', 403) RETURNING id",
  )
  .await
  .expect("the insert returned its id")
  .try_get::<i32>("", "id")
  .unwrap()
}

#[tokio::test]
async fn the_access_log_migration_repairs_a_serial_id_and_survives_its_round_trip() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let manager = SchemaManager::new(&db);
  let migration = entity::migration::access_log::Migration;

  db.execute_unprepared(LEGACY_ACCESS_LOGS).await.expect("legacy shape rebuilt");

  // The step this table was made for: the serial default must not survive against the identity,
  // and the duplicates must be gone before the primary key speaks.
  migration
    .up(&manager)
    .await
    .expect("up repairs a serial-id table that has no primary key");

  let (identity, default_expr, has_pk, owned_sequences) = shape(&db).await;
  assert_eq!(identity, "a", "the id is an identity column after up");
  assert_eq!(default_expr, None, "no separate default stands on the id after up");
  assert!(has_pk, "the primary key stands after up");
  assert_eq!(owned_sequences, 0, "the serial's sequence is gone after up");
  assert!(renumbered_oldest_first(&db).await, "ids are dense and distinct, oldest entry first");
  assert_eq!(next_generated_id(&db).await, 4, "the generator continues past the last row");
  assert!(
    db.execute_unprepared("INSERT INTO access_logs (id, method, code) VALUES (99, 'password', 200)")
      .await
      .is_err(),
    "a GENERATED ALWAYS id takes no explicit value — an explicit insert is how the old shape collided"
  );

  // Down leaves the column with no generator and the table with no constraint: the pre-migration
  // shape minus the serial, which a re-apply of up must take from there.
  migration.down(&manager).await.expect("down undoes the step");

  let (identity, default_expr, has_pk, owned_sequences) = shape(&db).await;
  assert_eq!(identity, "", "the id holds no identity after down");
  assert_eq!(default_expr, None, "the id holds no default after down");
  assert!(!has_pk, "the primary key is gone after down");
  assert_eq!(owned_sequences, 0, "no sequence stays bound to the id after down");

  migration.up(&manager).await.expect("up applies again after down undid it");

  let (identity, _, has_pk, _) = shape(&db).await;
  assert_eq!(identity, "a", "the id is an identity column again after the second up");
  assert!(has_pk, "the primary key stands again after the second up");
  assert_eq!(next_generated_id(&db).await, 5, "the second up restarts past every row on the table");

  // The shape an installation that archived its sign-in log under an older name keeps: the serial
  // default draws on a sequence another table owns and still uses. The repair takes the default
  // off the column and leaves that sequence standing for its owner.
  db.execute_unprepared(SHARED_SEQUENCE_ACCESS_LOGS)
    .await
    .expect("shared-sequence shape rebuilt");

  migration
    .up(&manager)
    .await
    .expect("up repairs a table whose default draws on a foreign-owned sequence");

  let (identity, default_expr, has_pk, owned_sequences) = shape(&db).await;
  assert_eq!(identity, "a", "the id is an identity column in the shared-sequence shape too");
  assert_eq!(default_expr, None, "the borrowed default is off the id");
  assert!(has_pk, "the primary key stands on the shared-sequence table");
  assert_eq!(owned_sequences, 0, "no sequence bound to the id survives its own repair");
  assert!(renumbered_oldest_first(&db).await, "the shared-sequence rows are renumbered too");

  let row = scalar(
    &db,
    r#"
    SELECT
      (SELECT count(*) FROM pg_class c
       JOIN pg_depend d ON d.objid = c.oid AND d.classid = 'pg_class'::regclass
         AND d.objsubid = 0 AND d.deptype = 'a'
       WHERE c.relname = 'user_logs_id_seq' AND c.relkind = 'S'
         AND d.refobjid = 'legacy_carrier'::regclass) AS still_owned,
      (SELECT count(*) FROM pg_class
       WHERE relname = 'user_logs_id_seq' AND relkind = 'S') AS still_there
    "#,
  )
  .await
  .expect("the catalog answered");
  assert_eq!(row.try_get::<i64>("", "still_there").unwrap(), 1, "the foreign-owned sequence survives");
  assert_eq!(row.try_get::<i64>("", "still_owned").unwrap(), 1, "it stays owned by its own table");

  let carrier = scalar(&db, "INSERT INTO legacy_carrier (note) VALUES ('still numbering') RETURNING id")
    .await
    .expect("the carrier still takes its numbers from the sequence");
  assert_eq!(carrier.try_get::<i32>("", "id").unwrap(), 1, "the shared sequence was never consumed");
}
