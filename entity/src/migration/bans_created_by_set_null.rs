use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// Removing the account that issued a ban leaves the ban in force and empties who issued it.
///
/// A ban is an operation record: an operator auditing the installation wants to see that the
/// disposition was written down even after the account that wrote it is gone. `bans.created_by`
/// records who acted, not who the ban shuts out — the row names its own account and its own
/// address — so removing the issuer keeps the row and only leaves that column naming nobody, and
/// the ban keeps shutting its target out. Cascading instead lifted the disposition silently, on
/// every account and address it still named, and left no record that it was ever written.
///
/// `bans.user_id` keeps its cascade: that column names who the ban shuts out, and a row shutting
/// out an account that no longer exists describes nothing, so those rows still follow the account.
/// The issuer column is already nullable, so a null is a value the constraint can write and the
/// column needs no turn of its own.
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
