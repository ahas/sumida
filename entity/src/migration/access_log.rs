use sea_orm::ConnectionTrait;
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
/// already has one and what stops it renumbering twice.
#[derive(DeriveMigrationName)]
pub struct Migration;

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

    db.execute_unprepared(
      r#"
      DO $$
      DECLARE
        next_id bigint;
        owned record;
      BEGIN
        IF EXISTS (
          SELECT 1 FROM pg_constraint
          WHERE conrelid = 'access_logs'::regclass AND contype = 'p'
        ) THEN
          RETURN;
        END IF;

        -- The column generates its own values, which is also why they cannot simply be rewritten.
        -- It comes off for the renumbering and goes back on afterwards, continuing past the last
        -- row rather than from one — starting over is how the collisions arose.
        --
        -- What the column arrives with is not fixed: a table made by sea-orm's
        -- `integer().auto_increment()` carries a serial-style default over a sequence the column
        -- owns, and one whose generator was removed elsewhere is a plain column or an identity.
        -- Every generator comes off first — ADD GENERATED refuses a column that still answers for
        -- its own values, and a sequence still owned by the column would survive beside the
        -- identity's own with nothing left to ever drop it.
        ALTER TABLE access_logs ALTER COLUMN id DROP IDENTITY IF EXISTS;
        ALTER TABLE access_logs ALTER COLUMN id DROP DEFAULT;

        FOR owned IN
          SELECT ns.nspname, s.relname
          FROM pg_depend d
          JOIN pg_class s ON s.oid = d.objid AND s.relkind = 'S'
          JOIN pg_namespace ns ON ns.oid = s.relnamespace
          WHERE d.classid = 'pg_class'::regclass
            AND d.objsubid = 0
            AND d.deptype = 'a'
            AND d.refobjid = 'access_logs'::regclass
            AND d.refobjsubid = (
              SELECT attnum FROM pg_attribute
              WHERE attrelid = 'access_logs'::regclass AND attname = 'id'
            )
        LOOP
          EXECUTE format('DROP SEQUENCE IF EXISTS %I.%I', owned.nspname, owned.relname);
        END LOOP;

        -- `ctid` is the only handle on a row here, precisely because `id` does not identify one.
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

        ALTER TABLE access_logs ADD PRIMARY KEY (id);
      END
      $$;
      "#,
    )
    .await?;

    Ok(())
  }

  /// The identifiers are not restored: the values this replaced identified nothing, so there is
  /// nothing to return to. Dropping the constraint, the generator however it stands, and the
  /// sequence it leaves unbound is enough to undo the shape.
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE access_logs DROP CONSTRAINT IF EXISTS access_logs_pkey;
      ALTER TABLE access_logs ALTER COLUMN id DROP IDENTITY IF EXISTS;
      ALTER TABLE access_logs ALTER COLUMN id DROP DEFAULT;
      DROP SEQUENCE IF EXISTS access_logs_id_seq;

      DROP INDEX IF EXISTS idx_access_logs__user_id__method__code__created_at;
      DROP INDEX IF EXISTS idx_access_logs__code__created_at;
      DROP INDEX IF EXISTS idx_access_logs__created_at;
      "#,
    )
    .await?;

    Ok(())
  }
}
