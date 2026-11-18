use e2e::migration::{MigrationTrait, SchemaManager};
use e2e::prelude::*;
use entity::migration::access_log::Migration;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::sync::Mutex;

/// Every test here reshapes the one `access_logs` table, so they take turns on it.
static ACCESS_LOGS: Mutex<()> = Mutex::const_new(());

/// A sign-in log whose `id` is a serial column with no primary key, and two entries sharing one id.
///
/// This is the shape the access log migration's renumbering step exists for: a table created with
/// an auto-incrementing integer, which PostgreSQL realises as a default drawing on a sequence the
/// column owns rather than as an identity, carrying no primary key, and with the sequence once set
/// back so that it handed out a number already taken. The entries are written out of their id
/// order so the renumbering has to follow `created_at` rather than the ids it found.
const SERIAL_ID_WITHOUT_PRIMARY_KEY: &str = r#"
  DROP TABLE access_logs;

  CREATE TABLE access_logs (
    id serial NOT NULL,
    user_id integer NOT NULL,
    method varchar NOT NULL,
    code integer NOT NULL,
    ip_address varchar NOT NULL DEFAULT '127.0.0.1',
    user_agent varchar NOT NULL DEFAULT 'tester',
    os_family varchar NOT NULL DEFAULT 'Other',
    device_family varchar NOT NULL DEFAULT 'Other',
    created_at timestamptz NOT NULL DEFAULT now()
  );

  INSERT INTO access_logs (id, user_id, method, code, created_at) VALUES
    (2, 1, 'password', 200, '2026-01-01T00:00:03Z'),
    (1, 1, 'password', 200, '2026-01-01T00:00:01Z'),
    (1, 1, 'password', 401, '2026-01-01T00:00:02Z');
"#;

/// The migration renumbers a serial-id sign-in log that has no primary key, gives it one, and comes
/// back down and up again on the table it left.
///
/// Every entry ends with its own id in the order the entries were written, the table holds its
/// primary key, and a new entry draws the next number past the last row rather than one already
/// taken. The same holds after `down` and a second `up`, which is the path a rollback and a
/// redeploy take over the table the first `up` produced.
#[tokio::test]
async fn a_serial_id_without_a_primary_key_is_renumbered_and_survives_a_round_trip() {
  let _turn = ACCESS_LOGS.lock().await;
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let manager = SchemaManager::new(&db);

  db.execute_unprepared(SERIAL_ID_WITHOUT_PRIMARY_KEY)
    .await
    .expect("build the serial-id sign-in log");

  Migration
    .up(&manager)
    .await
    .expect("up renumbers a serial-id sign-in log without a primary key");
  assert_renumbered(&db, &[1, 2, 3]).await;
  assert_eq!(insert_entry(&db, 1).await, 4, "a new entry draws the number past the last row");

  Migration.down(&manager).await.expect("down undoes the shape up left");
  Migration.up(&manager).await.expect("up runs again on the table down left");
  assert_renumbered(&db, &[1, 2, 3, 4]).await;
  assert_eq!(insert_entry(&db, 1).await, 5, "a new entry draws the number past the last row");
}

/// A sign-in log with no primary key whose `id` draws on another table's sequence, and two entries
/// sharing one id.
///
/// A table renamed or rebuilt beside an older one can keep a default on that older table's serial
/// sequence, which the column does not own and the older table still draws on.
const BORROWED_SEQUENCE_WITHOUT_PRIMARY_KEY: &str = r#"
  DROP TABLE access_logs;
  DROP TABLE IF EXISTS user_logs;

  CREATE TABLE user_logs (id serial NOT NULL);

  CREATE TABLE access_logs (
    id integer NOT NULL DEFAULT nextval('user_logs_id_seq'),
    user_id integer NOT NULL,
    method varchar NOT NULL,
    code integer NOT NULL,
    ip_address varchar NOT NULL DEFAULT '127.0.0.1',
    user_agent varchar NOT NULL DEFAULT 'tester',
    os_family varchar NOT NULL DEFAULT 'Other',
    device_family varchar NOT NULL DEFAULT 'Other',
    created_at timestamptz NOT NULL DEFAULT now()
  );

  INSERT INTO access_logs (id, user_id, method, code, created_at) VALUES
    (1, 1, 'password', 200, '2026-01-01T00:00:01Z'),
    (1, 1, 'password', 401, '2026-01-01T00:00:02Z');
"#;

/// The migration takes a borrowed default off the sign-in log and leaves the sequence it borrowed
/// to the table that owns it.
///
/// The log is renumbered and keyed as a serial one is, and the other table still numbers its own
/// rows from its sequence afterwards, across a `down` and a second `up` as well.
#[tokio::test]
async fn a_borrowed_sequence_is_left_to_the_table_that_owns_it() {
  let _turn = ACCESS_LOGS.lock().await;
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let manager = SchemaManager::new(&db);

  db.execute_unprepared(BORROWED_SEQUENCE_WITHOUT_PRIMARY_KEY)
    .await
    .expect("build the sign-in log on a borrowed sequence");

  Migration
    .up(&manager)
    .await
    .expect("up renumbers a sign-in log whose default draws on another table's sequence");
  assert_renumbered(&db, &[1, 2]).await;
  assert_eq!(insert_entry(&db, 1).await, 3, "a new entry draws the number past the last row");

  Migration.down(&manager).await.expect("down undoes the shape up left");
  Migration.up(&manager).await.expect("up runs again on the table down left");
  assert_renumbered(&db, &[1, 2, 3]).await;

  let owner_draws = db
    .query_one(Statement::from_string(
      DbBackend::Postgres,
      "INSERT INTO user_logs DEFAULT VALUES RETURNING id",
    ))
    .await
    .expect("the owning table still draws on its sequence");
  assert!(owner_draws.is_some(), "the owning table numbers its own row");

  db.execute_unprepared("DROP TABLE user_logs").await.expect("drop the owning table");
}

/// The sign-in log `init` creates comes back down and up again with its key and its generator.
///
/// `init` names the key itself and gives the column a serial generator, so `up` finds a key and
/// leaves the table as it is. `down` has to take off that key, under the name `init` gave it, or
/// the next `up` finds it still standing, skips its step, and leaves a column with nothing to
/// generate its values.
#[tokio::test]
async fn the_table_init_creates_survives_a_round_trip() {
  let _turn = ACCESS_LOGS.lock().await;
  bootstrap::init_once().await;
  TestDb::reset_database().await.expect("migrate a fresh database");
  let db = infra::db::connect().await;
  let manager = SchemaManager::new(&db);

  let user = TestUser::create(&db).await.expect("seed an account").model.id;
  for expected in 1..=3 {
    assert_eq!(insert_entry(&db, user).await, expected, "init's serial column numbers the entries");
  }

  Migration.down(&manager).await.expect("down undoes the shape init and up left");
  Migration.up(&manager).await.expect("up runs again on the table down left");
  assert_renumbered(&db, &[1, 2, 3]).await;
  assert_eq!(insert_entry(&db, user).await, 4, "a new entry draws the number past the last row");
}

/// Asserts that the entries, read in the order they were written, carry `expected` as their ids,
/// and that the table holds its primary key on `id` under the name `init` gives it.
async fn assert_renumbered(db: &DatabaseConnection, expected: &[i32]) {
  let rows = db
    .query_all(Statement::from_string(
      DbBackend::Postgres,
      "SELECT id FROM access_logs ORDER BY created_at, id",
    ))
    .await
    .expect("read the sign-in log");
  let ids: Vec<i32> = rows.iter().map(|row| row.try_get("", "id").expect("an integer id")).collect();
  assert_eq!(ids, expected, "every entry carries its own id, in the order the entries were written");

  let primary_key: Option<String> = db
    .query_one(Statement::from_string(
      DbBackend::Postgres,
      "SELECT conname::text AS name FROM pg_constraint \
       WHERE conrelid = 'access_logs'::regclass AND contype = 'p'",
    ))
    .await
    .expect("read the sign-in log's constraints")
    .map(|row| row.try_get("", "name").expect("a constraint name"));
  assert_eq!(
    primary_key.as_deref(),
    Some("pk_access_logs__id"),
    "the sign-in log holds its primary key"
  );
}

/// Writes one entry for `user`, later than every other, with no id of its own and returns the one it
/// drew.
async fn insert_entry(db: &DatabaseConnection, user: i32) -> i32 {
  db.query_one(Statement::from_string(
    DbBackend::Postgres,
    format!(
      "INSERT INTO access_logs \
         (user_id, method, code, ip_address, user_agent, os_family, device_family, created_at) \
       VALUES ({user}, 'password', 200, '127.0.0.1', 'tester', 'Other', 'Other', \
         clock_timestamp() + interval '1 day') \
       RETURNING id"
    ),
  ))
  .await
  .expect("write an entry")
  .expect("the insert returns its row")
  .try_get("", "id")
  .expect("an integer id")
}
