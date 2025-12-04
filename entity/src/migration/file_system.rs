use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// files
#[derive(DeriveIden)]
enum Files {
  Table,
  Uuid,
  UserId,
  OriginalName,
  Dir,
  Name,
  Uri,
  Size,
  MimeType,
  Hash,
  Tags,
  Refs,
  Status,
  ExpiresAt,
  CreatedAt,
  UpdatedAt,
  DeletedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // files
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Files::Table)
          .col(uuid(Files::Uuid).default(Expr::cust("uuid_generate_v4()")))
          .col(integer_null(Files::UserId))
          .col(string(Files::OriginalName))
          .col(string(Files::Dir))
          .col(string(Files::Name))
          .col(string_null(Files::Uri))
          .col(big_integer(Files::Size))
          .col(string(Files::MimeType))
          .col(string_null(Files::Hash))
          .col(array_null(Files::Tags, ColumnType::String(StringLen::None)))
          .col(integer(Files::Refs).default(0))
          .col(string(Files::Status).default("pending"))
          .col(timestamp_with_time_zone_null(Files::ExpiresAt))
          .col(timestamp_with_time_zone(Files::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Files::UpdatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone_null(Files::DeletedAt))
          .primary_key(Index::create().name("pk_files__uuid").col(Files::Uuid))
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "files").await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    drop_table(manager, Files::Table).await?;

    Ok(())
  }
}
