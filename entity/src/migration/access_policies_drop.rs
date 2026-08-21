use sea_orm::{ConnectionTrait, DbErr, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

use super::helper::drop_table;

/// Drops `access_policies`.
///
/// Nothing resolves a permission against that table. The rules read `relation_tuples`, the two
/// tombstones (`rebac_backfill`, `rebac_mirror`) only assert the table is empty and take the mirror
/// trigger off it, and no read or write path names it. What is left is a table whose name means
/// "permissions" standing in the schema, inviting a row that grants nothing — a denial nobody can
/// account for, because the row looks right and is never read.
///
/// The safety check is the one `rebac_backfill` already states, and it has to stay the same check:
/// a row that is not the `* | *` catch-all is a grant somebody would lose, so the migration refuses
/// rather than dropping it. The catch-all is excluded for the reason it is excluded there — the
/// power it stood for is `platform:root#superuser`, whose flag branch reads `users.is_admin`, so it
/// never had a tuple behind it to lose.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum AccessPolicies {
  Table,
}

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

/// How many rows carry a grant somebody would lose. Same predicate as `rebac_backfill`'s, because
/// it is the same question: the two must not disagree about which row is safe to walk past.
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
      tracing::info!("access_policies_drop: no access_policies table — nothing to drop");
      return Ok(());
    }

    let rows = untranslated_rows(db).await?;
    if rows > 0 {
      return Err(DbErr::Custom(format!(
        "access_policies_drop refuses to run: access_policies holds {rows} row(s) that carry a \
         grant, and nothing in this checkout reads them. Dropping the table would take those \
         grants with it, silently. Write them as relation tuples, clear the table, and run the \
         migration again."
      )));
    }

    drop_table(manager, AccessPolicies::Table).await?;
    tracing::info!("access_policies_drop: access_policies held nothing and is gone");

    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    // Nothing to restore. `up` refuses while the table holds a grant, so what it drops is an empty
    // table that nothing reads; re-creating it would put back the trap and none of the data.
    Ok(())
  }
}
