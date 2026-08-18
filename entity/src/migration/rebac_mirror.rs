use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// **Tombstone.** This migration once installed a trigger on `access_policies` that mirrored every
/// write into `relation_tuples`, so the two stores stayed in step while the legacy table was still
/// the authority and things were still writing to it.
///
/// Nothing writes to that table now, and no database this ran against still has it. The mapping the
/// trigger carried was the backfill's, re-expressed one row at a time, and it went where the
/// backfill's went — it named the resource vocabulary of whichever services existed when it was
/// written, which is precisely what a framework must not carry.
///
/// **The file survives because sea-orm tracks migrations by file stem**, so removing it would make
/// `migration up` refuse on every database that has already applied it.
///
/// `up` now drops the trigger and its functions if they are still there, which is the correct
/// end state and is a no-op everywhere the table is already gone.
#[derive(DeriveMigrationName)]
pub struct Migration;

async fn table_exists<C: ConnectionTrait>(db: &C, name: &str) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Present {
    present: bool,
  }

  let row = Present::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "SELECT to_regclass($1) IS NOT NULL AS present",
    [name.into()],
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

/// The trigger and the two functions the mirror installed.
///
/// `DROP TRIGGER` names a table, so it is issued only when the table is there; the functions are
/// free-standing and can go either way.
const DROP_MIRROR: &str = r#"
DROP TRIGGER IF EXISTS trg_rebac_mirror ON access_policies;
DROP FUNCTION IF EXISTS tg_rebac_mirror_access_policy();
DROP FUNCTION IF EXISTS rebac_mirror_map(text, text, text[], text, text);
DROP FUNCTION IF EXISTS rebac_mirror_subject(text);
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if !table_exists(db, "access_policies").await? {
      tracing::info!("rebac_mirror: no access_policies table — nothing to mirror");
      return Ok(());
    }

    db.execute_unprepared(DROP_MIRROR).await?;
    tracing::info!("rebac_mirror: the mirror trigger is not installed");

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if !table_exists(db, "access_policies").await? {
      return Ok(());
    }

    db.execute_unprepared(DROP_MIRROR).await?;
    db.execute_unprepared("DELETE FROM relation_tuples WHERE created_by = 'access_policies';")
      .await?;

    Ok(())
  }
}
