use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// An account's removal takes its sign-in log with it.
///
/// `access_logs.user_id` is NOT NULL, so a log cannot outlive its account: removing the account
/// would otherwise fail on the log's foreign key instead of removing both. Every other table
/// hanging off an account already removes its rows with it, and a sign-in entry records that
/// account's own attempts, so the log follows the same rule.
///
/// The constraint is dropped and recreated because PostgreSQL has no `ALTER CONSTRAINT` for a
/// foreign key's delete action. This migration only needs `access_logs` and `users`, both from
/// `init`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE access_logs DROP CONSTRAINT IF EXISTS fk_access_logs__user_id;
      ALTER TABLE access_logs
        ADD CONSTRAINT fk_access_logs__user_id
        FOREIGN KEY (user_id) REFERENCES users (id)
        ON DELETE CASCADE ON UPDATE CASCADE;
      "#,
    )
    .await?;

    Ok(())
  }

  /// Puts the foreign key back the way it was before this migration.
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE access_logs DROP CONSTRAINT IF EXISTS fk_access_logs__user_id;
      ALTER TABLE access_logs
        ADD CONSTRAINT fk_access_logs__user_id
        FOREIGN KEY (user_id) REFERENCES users (id)
        ON DELETE SET NULL ON UPDATE CASCADE;
      "#,
    )
    .await?;

    Ok(())
  }
}
