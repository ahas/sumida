use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// The index the sign-in lockout reads by.
///
/// Deciding whether an account has run up a streak of password failures takes three reads of the
/// sign-in log, and every one of them narrows by account, then by method and result, then walks
/// backwards through time. The table's other indexes lead with `created_at` or with `code`, so none
/// of them can seek to one account — each read scanned the whole table, on the password-mismatch
/// path, which is the one an attacker drives hardest.
///
/// The columns are in the order the reads use them: the three equalities first, the range and the
/// ordering last, so the planner seeks to the account's own newest rows and stops as soon as it has
/// the few it asked for.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      CREATE INDEX IF NOT EXISTS idx_access_logs__user_id__method__code__created_at
        ON access_logs (user_id, method, code, created_at DESC);
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      DROP INDEX IF EXISTS idx_access_logs__user_id__method__code__created_at;
      "#,
    )
    .await?;

    Ok(())
  }
}
