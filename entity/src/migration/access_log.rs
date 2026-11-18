use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

/// The sign-in log's identifier and the indexes every read of it needs.
///
/// One row per authentication attempt, so it outgrows every other table in the installation and is
/// the one most often read by recency. Three shapes read it, and it had nothing to seek by:
///
/// * newest-first, which the list's own default ordering and the dashboard's windowed aggregates
///   both want — `created_at DESC`;
/// * one value of `code` over a window, which the refusal reads ask for — `(code, created_at DESC)`,
///   in that order so the planner seeks rather than filters;
/// * one account's own recent attempts, narrowed by method and result, which is how the sign-in
///   lockout decides whether a password has been missed too often — the three equalities first, the
///   range and the ordering last. Without it that read scanned the whole table on the
///   password-mismatch path, which is the one an attacker drives hardest.
///
/// The identifier is the other half. The entity declares `id` as an auto-incrementing primary key
/// and the column does generate its own values, but the table carries no such constraint — so when
/// the generator was once set back to the beginning, the numbers it handed out collided with
/// numbers already there and nothing refused them. Every read by id then picked an arbitrary row
/// among the duplicates, and a delete by id removed all of them.
///
/// Rows are renumbered oldest first, so the order the identifiers imply matches the order the
/// entries were written. Nothing references these values — no foreign key points here and no screen
/// fetches a single entry — so renumbering costs nothing outside this table. The step is skipped
/// wherever a primary key already stands, which is what makes it safe against a database that
/// already has one and what stops it renumbering twice. Where it runs, the table ends with an
/// identity column and a primary key named as `init` names it, `pk_access_logs__id`.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Takes the generator off `access_logs.id`, in either form it may stand in.
///
/// A table created with an auto-incrementing integer has a serial column: a default drawing on a
/// sequence the column owns. A table this migration renumbered has an identity instead. An
/// identity cannot be added while a default stands, and a default cannot be dropped from an
/// identity column, so both forms come off, the identity first. Only a sequence the column owns is
/// dropped: a default can also draw on another table's sequence, which that table still uses.
const DROP_ID_GENERATOR: &str = r#"
  DO $$
  DECLARE
    serial_sequence text;
  BEGIN
    ALTER TABLE access_logs ALTER COLUMN id DROP IDENTITY IF EXISTS;
    ALTER TABLE access_logs ALTER COLUMN id DROP DEFAULT;

    serial_sequence := pg_get_serial_sequence('access_logs', 'id');
    IF serial_sequence IS NOT NULL THEN
      EXECUTE format('DROP SEQUENCE %s', serial_sequence);
    END IF;
  END
  $$;
"#;

/// Drops the primary key of `access_logs`, whatever it is named: `init` and this migration name it
/// `pk_access_logs__id`, and a key added without a name carries PostgreSQL's default instead.
const DROP_PRIMARY_KEY: &str = r#"
  DO $$
  DECLARE
    primary_key name;
  BEGIN
    SELECT conname INTO primary_key FROM pg_constraint
    WHERE conrelid = 'access_logs'::regclass AND contype = 'p';

    IF primary_key IS NOT NULL THEN
      EXECUTE format('ALTER TABLE access_logs DROP CONSTRAINT %I', primary_key);
    END IF;
  END
  $$;
"#;

/// Gives every entry its own id, oldest first, then an identity continuing past the last row
/// rather than from one — starting over is how the collisions arose — and the primary key.
///
/// `ctid` is the only handle on a row here, precisely because `id` does not identify one.
const RENUMBER: &str = r#"
  DO $$
  DECLARE
    next_id bigint;
  BEGIN
    WITH renumbered AS (
      SELECT ctid AS row_id, row_number() OVER (ORDER BY created_at, ctid) AS ordinal
      FROM access_logs
    )
    UPDATE access_logs
    SET id = renumbered.ordinal
    FROM renumbered
    WHERE access_logs.ctid = renumbered.row_id;

    SELECT COALESCE(max(id), 0) + 1 INTO next_id FROM access_logs;

    EXECUTE format(
      'ALTER TABLE access_logs ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (RESTART WITH %s)',
      next_id
    );

    ALTER TABLE access_logs ADD CONSTRAINT pk_access_logs__id PRIMARY KEY (id);
  END
  $$;
"#;

async fn has_primary_key<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
  let key = db
    .query_one(Statement::from_string(
      db.get_database_backend(),
      "SELECT 1 FROM pg_constraint WHERE conrelid = 'access_logs'::regclass AND contype = 'p'",
    ))
    .await?;

  Ok(key.is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      CREATE INDEX IF NOT EXISTS idx_access_logs__created_at
        ON access_logs (created_at DESC);

      CREATE INDEX IF NOT EXISTS idx_access_logs__code__created_at
        ON access_logs (code, created_at DESC);

      CREATE INDEX IF NOT EXISTS idx_access_logs__user_id__method__code__created_at
        ON access_logs (user_id, method, code, created_at DESC);
      "#,
    )
    .await?;

    if has_primary_key(db).await? {
      return Ok(());
    }

    // The column generates its own values, which is also why they cannot simply be rewritten.
    db.execute_unprepared(DROP_ID_GENERATOR).await?;
    db.execute_unprepared(RENUMBER).await?;

    Ok(())
  }

  /// The identifiers are not restored: the values this replaced identified nothing, so there is
  /// nothing to return to. Dropping the primary key, whichever stands, and the generator is enough
  /// to undo the shape; the next `up` then finds no key and gives the table its identity and key
  /// again.
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(DROP_PRIMARY_KEY).await?;
    db.execute_unprepared(DROP_ID_GENERATOR).await?;
    db.execute_unprepared(
      r#"
      DROP INDEX IF EXISTS idx_access_logs__user_id__method__code__created_at;
      DROP INDEX IF EXISTS idx_access_logs__code__created_at;
      DROP INDEX IF EXISTS idx_access_logs__created_at;
      "#,
    )
    .await?;

    Ok(())
  }
}
