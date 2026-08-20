use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// The foreign key that makes an api key's holder a fact rather than a hope.
///
/// `api_keys.user_id` is declared `NOT NULL` and cascades from `users`, and every read of the
/// resource is built on that: the list and the single-record payload project the holder as a
/// required part of the row, and the row scope decides who may see a key by looking at who holds
/// it. A database that carries the column without the constraint admits rows whose holder is gone,
/// and such a row answers neither question.
///
/// Recreating the constraint is therefore the repair, and it can only be created once no row
/// violates it — the surviving orphans go first, together with the grants they still hold, the way
/// a deletion through the service revokes them before removing the key.
///
/// Both statements are safe to run again: an orphan sweep over a clean table matches nothing, and
/// the constraint is dropped by name before it is created.
#[derive(DeriveMigrationName)]
pub struct Migration;

const CONSTRAINT: &str = "fk_api_keys__user_id";

/// Whether this database keeps the relation tuples at all.
///
/// A runtime probe rather than a migration ordering assumption: the tuples are created by a
/// migration of their own, and a database this one runs against may not have reached it yet.
async fn has_relation_tuples<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Present {
    present: bool,
  }

  let row = Present::find_by_statement(Statement::from_string(
    db.get_database_backend(),
    "SELECT to_regclass('relation_tuples') IS NOT NULL AS present".to_owned(),
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if has_relation_tuples(db).await? {
      // Subject side only, which is what a key's own revocation touches. `api_keys` has no cascade
      // into the tuples, so nothing removes these once the key row is gone.
      db.execute_unprepared(
        r#"
        DELETE FROM relation_tuples t
          USING api_keys k
          WHERE t.subject_type = 'apikey'
            AND t.subject_id = k.key
            AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = k.user_id);
        "#,
      )
      .await?;
    }

    db.execute_unprepared(&format!(
      r#"
      DELETE FROM api_keys k WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = k.user_id);

      ALTER TABLE api_keys ALTER COLUMN user_id SET NOT NULL;

      ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS {CONSTRAINT};
      ALTER TABLE api_keys ADD CONSTRAINT {CONSTRAINT}
        FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE ON UPDATE CASCADE;
      "#
    ))
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // Only the constraint. The rows it could not admit are gone for good.
    db.execute_unprepared(&format!("ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS {CONSTRAINT};"))
      .await?;

    Ok(())
  }
}
