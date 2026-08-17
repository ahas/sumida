use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// The sumida-side columns and indexes the compiled rule set reads (design §3.4, §3.5).
///
/// Purely additive, and — like everything else in this phase — read by nothing yet. Two changes,
/// both because a rule needs a fact the schema does not currently record:
///
/// * `boards.organization_id`, **nullable on purpose**. It is how "boards stay general-purpose"
///   lives in the model instead of in a config flag: a public-board deployment leaves it NULL and
///   the `org->manage_board` branch of the board rule is statically dead, while a business
///   deployment sets it and organization managers reach their own boards. A `NOT NULL` column with
///   a default would force every deployment into the second shape.
/// * `idx_users__is_admin`, so R1's `platform#flagged` branch — an uncorrelated
///   `EXISTS (SELECT 1 FROM users WHERE is_admin AND id = ANY($2))` that Postgres hoists into an
///   InitPlan — is index-backed rather than a sequential scan once per statement.
///
/// The remaining columns of §3.4/§3.5 belong to trainer (`tasks.owner_user_id`, `tasks.owner_scope`,
/// `jobs.owner_key`, `agents.organization_id`, `cloud_instances.owner_user_id`) and land in trainer's
/// own `src/migration/joy_service_init.rs`, per trainer's convention. Until they do, `Authz::boot`
/// reports trainer's definitions as drift-blocked against a trainer database rather than activating a rule
/// that reads a column which does not exist.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Whether this database has the board tables at all.
///
/// A runtime probe rather than `#[cfg(feature = "board")]`, for the reason `rebac.rs` records:
/// Cargo unifies features across the dependency graph, so `entity` can be compiled with `board` on
/// for a build whose database has no board tables — site-app is exactly that.
async fn has_boards<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Present {
    present: bool,
  }

  let row = Present::find_by_statement(Statement::from_string(
    db.get_database_backend(),
    "SELECT to_regclass('boards') IS NOT NULL AS present".to_owned(),
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      -- R1's flag branch. Partial, because a query only ever asks "is this account an admin"; the
      -- non-admin rows would be dead weight in the index.
      CREATE INDEX IF NOT EXISTS idx_users__is_admin ON users (id) WHERE is_admin;
      "#,
    )
    .await?;

    if !has_boards(db).await? {
      return Ok(());
    }

    db.execute_unprepared(
      r#"
      ALTER TABLE boards
        ADD COLUMN IF NOT EXISTS organization_id INTEGER NULL
          REFERENCES organizations(id) ON DELETE SET NULL;

      -- Partial: NULL means "this board is not organization-scoped", which is the common case in a
      -- public-board deployment and never a value the rule looks up.
      CREATE INDEX IF NOT EXISTS idx_boards__organization_id
        ON boards (organization_id) WHERE organization_id IS NOT NULL;

      COMMENT ON COLUMN boards.organization_id IS
        'Optional owning organization. NULL leaves the board outside organization scoping; the rule''s org branch is then dead.';
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if has_boards(db).await? {
      db.execute_unprepared(
        r#"
        DROP INDEX IF EXISTS idx_boards__organization_id;
        ALTER TABLE boards DROP COLUMN IF EXISTS organization_id;
        "#,
      )
      .await?;
    }

    db.execute_unprepared("DROP INDEX IF EXISTS idx_users__is_admin;").await?;

    Ok(())
  }
}
