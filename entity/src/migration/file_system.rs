use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// files
#[derive(DeriveIden)]
enum Files {
  Table,
  Id,
  UserId,
  OriginalName,
  Path,
  Uri,
  Size,
  MimeType,
  Hash,
  Tags,
  Status,
  IsTemporary,
  ExpiresAt,
  CreatedAt,
  UpdatedAt,
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
          .col(integer(Files::Id).auto_increment())
          .col(integer_null(Files::UserId))
          .col(string(Files::OriginalName))
          .col(string(Files::Path))
          .col(string_null(Files::Uri))
          .col(big_integer(Files::Size))
          .col(string(Files::MimeType))
          .col(string_null(Files::Hash))
          .col(array_null(Files::Tags, ColumnType::String(StringLen::None)))
          .col(string(Files::Status).default("pending"))
          .col(boolean(Files::IsTemporary).default(false))
          .col(timestamp_with_time_zone_null(Files::ExpiresAt))
          .col(timestamp_with_time_zone(Files::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Files::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_files__id").col(Files::Id))
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
