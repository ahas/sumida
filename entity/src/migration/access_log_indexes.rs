use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// Indexes for the sign-in log.
///
/// The table carries one row per authentication attempt, so it outgrows every other table in the
/// installation and is the one most often read by recency. It had no index of any kind, which left
/// the list's own default ordering and the dashboard's two aggregates reading every row.
///
/// * `created_at DESC` backs both the newest-first list and the windowed aggregates, which bound
///   themselves by date.
/// * `(code, created_at DESC)` backs the refusal reads specifically. They ask for one value of
///   `code` over a window, and a composite in that order lets the planner seek rather than filter.
///
/// A primary key is deliberately absent here: `id` carries duplicates and no sequence, so the
/// constraint cannot be added without first deciding what the identifier should be. That is a data
/// repair, not an index.
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
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      DROP INDEX IF EXISTS idx_access_logs__code__created_at;
      DROP INDEX IF EXISTS idx_access_logs__created_at;
      "#,
    )
    .await?;

    Ok(())
  }
}
