use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// The moment an account last reset the credentials issued to it.
///
/// A password is changed because the ones already handed out may be in somebody else's hands, and
/// neither a token nor a session cookie can be told apart from the same one held by whoever took
/// it. So the account records when it last reset them, and the sign-in paths refuse every credential
/// established up to that moment. NULL is an account that has never reset them, which is what
/// every account starts as.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE users
        ADD COLUMN IF NOT EXISTS credentials_reset_at TIMESTAMPTZ NULL;

      COMMENT ON COLUMN users.credentials_reset_at IS
        'When this account last reset its credentials. Every token and session established up to it is refused; NULL means it never has.';
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE users DROP COLUMN IF EXISTS credentials_reset_at;
      "#,
    )
    .await?;

    Ok(())
  }
}
