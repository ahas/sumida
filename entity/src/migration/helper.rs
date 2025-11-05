use sea_orm::sea_query::{IntoTableRef, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::SchemaManager;

pub async fn drop_table<'a, T: IntoTableRef>(
  manager: &SchemaManager<'a>,
  table_iden: T,
) -> Result<(), DbErr> {
  manager
    .drop_table(Table::drop().if_exists().cascade().table(table_iden).to_owned())
    .await
}

pub async fn attach_timestamp_auto_update<C: ConnectionTrait>(
  db: &C,
  table_name: &str,
) -> Result<(), DbErr> {
  db.execute_unprepared(&format!("SELECT manage_updated_at('{table_name}');").to_owned())
    .await?;
  Ok(())
}

pub async fn detach_timestamp_auto_update<C: ConnectionTrait>(
  db: &C,
  table_name: &str,
) -> Result<(), DbErr> {
  db.execute_unprepared(&format!("DROP TRIGGER IF EXISTS set_updated_at ON {table_name};").to_owned())
    .await?;
  Ok(())
}
