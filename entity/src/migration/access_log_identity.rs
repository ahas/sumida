use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// Gives the sign-in log the identifier its model already assumes.
///
/// The entity declares `id` as an auto-incrementing primary key, and the column does generate its
/// own values, but the table carries no such constraint — so when the generator was once set back
/// to the beginning, the numbers it handed out collided with numbers already in the table and
/// nothing refused them. Every read by id therefore picked an arbitrary row among the duplicates,
/// and a delete by id removed all of them.
///
/// Rows are renumbered oldest first, so the order the identifiers imply matches the order the
/// entries were written. Nothing references these values — no foreign key points here and no screen
/// fetches a single entry — so renumbering costs nothing outside this table.
///
/// The whole step is skipped where a primary key already stands, which is what makes it safe to run
/// against a database created after this landed, and what stops it from renumbering a second time.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      DO $$
      DECLARE
        next_id bigint;
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
        ALTER TABLE access_logs ALTER COLUMN id DROP IDENTITY IF EXISTS;

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
  /// nothing to return to. Dropping the constraint and the default is enough to undo the shape.
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE access_logs DROP CONSTRAINT IF EXISTS access_logs_pkey;
      ALTER TABLE access_logs ALTER COLUMN id DROP DEFAULT;
      DROP SEQUENCE IF EXISTS access_logs_id_seq;
      "#,
    )
    .await?;

    Ok(())
  }
}
