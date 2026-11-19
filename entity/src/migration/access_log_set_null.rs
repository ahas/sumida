use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// An account's removal leaves its sign-in rows behind, naming no account.
///
/// Sign-in entries are operation records: an operator auditing the installation wants to see that
/// an account existed and signed in even after it is gone. So `access_logs.user_id` turns nullable
/// and its foreign key answers a delete with `SET NULL` instead of a cascade. Reads that take the
/// whole row see the absent account as `NULL`; the log's own columns — method, code, address,
/// agent, device — still speak.
///
/// The constraint is dropped and recreated because PostgreSQL has no `ALTER CONSTRAINT` for a
/// foreign key's delete action, and the nullable turn precedes it so any already-orphaned row
/// survives the rebuild. This migration only needs `access_logs` and `users`, both from `init`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE access_logs ALTER COLUMN user_id DROP NOT NULL;

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

  /// Ends the keeping: rows left without an account are the ones only this shape made possible, so
  /// they go before the column is put back under `NOT NULL`, and the key goes back to cascading.
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      DELETE FROM access_logs WHERE user_id IS NULL;

      ALTER TABLE access_logs ALTER COLUMN user_id SET NOT NULL;

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
}
