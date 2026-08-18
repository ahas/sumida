use sea_orm::{ConnectionTrait, DbErr, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// **Tombstone.** This migration once translated the legacy `access_policies` table into
/// `relation_tuples`, row by row, refusing anything the mapping did not cover rather than guessing.
///
/// The table it read is gone from every database this ran against, and the translation went with
/// it: it was a one-way mapping out of a store nothing reads any more, written against the
/// vocabulary of whichever services happened to exist when it was written. Keeping the code would
/// mean keeping that vocabulary.
///
/// **The file survives because sea-orm tracks migrations by file stem.** The version string in
/// `seaql_migrations` *is* `rebac_backfill`, and `migration up` refuses to run at all when an
/// applied version has no file behind it. Deleting the module would therefore break every database
/// this has already run on — which is all of them.
///
/// What it still does is the one thing that would otherwise be a silent loss: if a database arrives
/// holding policy rows that were never translated — an old dump restored onto a fresh checkout — it
/// refuses rather than walking past them. The migration's own invariant was that a policy row is
/// either translated or reported and there is no third outcome; with the translation gone,
/// reporting is all that is left, and it has to be loud.
#[derive(DeriveMigrationName)]
pub struct Migration;

async fn table_exists<C: ConnectionTrait>(db: &C, name: &str) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Exists {
    present: bool,
  }

  let row = Exists::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "SELECT to_regclass($1) IS NOT NULL AS present",
    [name.into()],
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

/// How many rows carry a grant somebody would lose.
///
/// The catch-all (`namespace = '*' AND object = '*'`) is excluded: it was superseded by
/// `platform:root#superuser`, whose flag branch reads `users.is_admin` itself, so it never had a
/// tuple to lose. It is the one shape a freshly migrated database legitimately arrives with, seeded
/// by a consumer's own initial migration.
async fn untranslated_rows<C: ConnectionTrait>(db: &C) -> Result<i64, DbErr> {
  #[derive(FromQueryResult)]
  struct Count {
    rows: i64,
  }

  let row = Count::find_by_statement(Statement::from_string(
    db.get_database_backend(),
    "SELECT count(*) AS rows FROM access_policies \
     WHERE NOT (namespace = '*' AND object = '*')"
      .to_owned(),
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.rows).unwrap_or(0))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if !table_exists(db, "access_policies").await? {
      tracing::info!("rebac_backfill: no access_policies table — nothing to translate");
      return Ok(());
    }

    let rows = untranslated_rows(db).await?;
    if rows > 0 {
      return Err(DbErr::Custom(format!(
        "rebac_backfill refuses to run: access_policies holds {rows} row(s) that would have been \
         translated into relation tuples, and the translation no longer exists in this checkout. \
         Continuing would drop those grants silently. Clear the table, or run the translation from \
         a checkout that still has it, and then run the migration again."
      )));
    }

    tracing::info!("rebac_backfill: access_policies holds nothing to translate");
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // Only what this migration wrote. `created_by` is 'migration' for every backfilled tuple and
    // nothing else writes that value, so a tuple created by hand after the fact survives a
    // rollback.
    db.execute_unprepared("DELETE FROM relation_tuples WHERE created_by = 'migration';")
      .await?;

    Ok(())
  }
}
