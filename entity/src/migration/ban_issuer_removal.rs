use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// A ban stays in force when the account that issued it is removed.
///
/// `bans.created_by` records who acted, not who the ban applies to: the row names its own account
/// and its own address, and it holds until it is lifted or its term runs out. So removing the
/// issuer must leave the row standing and only empty the column that names the issuer — the column
/// is nullable, which is what makes a null the value the constraint can write. Letting the row go
/// instead would lift the disposition silently, on every account and address it still names, and
/// would erase the record that it was ever written.
///
/// `bans.user_id` keeps its own rule: that column names who the ban shuts out, and a row shutting
/// out an account that no longer exists describes nothing, so those rows still follow the account.
///
/// The constraint is dropped and recreated because PostgreSQL has no `ALTER CONSTRAINT` for a
/// foreign key's delete action. This migration only needs `bans` and `users`, both from `init`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE bans DROP CONSTRAINT IF EXISTS fk_bans__created_by;
      ALTER TABLE bans
        ADD CONSTRAINT fk_bans__created_by
        FOREIGN KEY (created_by) REFERENCES users (id)
        ON DELETE SET NULL ON UPDATE CASCADE;
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
      ALTER TABLE bans DROP CONSTRAINT IF EXISTS fk_bans__created_by;
      ALTER TABLE bans
        ADD CONSTRAINT fk_bans__created_by
        FOREIGN KEY (created_by) REFERENCES users (id)
        ON DELETE CASCADE ON UPDATE CASCADE;
      "#,
    )
    .await?;

    Ok(())
  }
}
