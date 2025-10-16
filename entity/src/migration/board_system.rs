use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// board_categories
#[derive(DeriveIden)]
enum BoardCategories {
  Table,
  Id,
  ParentId,
  Name,
  Description,
  CreatedAt,
  UpdatedAt,
}

/// boards
#[derive(DeriveIden)]
enum Boards {
  Table,
  Id,
  CategoryId,
  Name,
  Description,
  CanUploadFile,
  MaxFileSize,
  MaxFileCount,
  AllowedFileTypes,
  ReactionVisibility,
  ViewerVisibility,
  IsLogged,
  IsEnabled,
  CreatedAt,
  UpdatedAt,
  DeletedAt,
}

/// board_posts
#[derive(DeriveIden)]
enum BoardPosts {
  Table,
  Id,
  ParentId,
  BoardId,
  UserId,
  Kind,
  Status,
  Revision,
  Title,
  CompressionType,
  ContentType,
  Content,
  Text,
  Tags,
  Views,
  Reactions,
  PublishedAt,
  ScheduledAt,
  CreatedAt,
  UpdatedAt,
  DeletedAt,
}

/// board_post_likes
#[derive(DeriveIden)]
enum BoardPostReactions {
  Table,
  Id,
  PostId,
  UserId,
  Value,
  CreatedAt,
}

/// board_post_views
#[derive(DeriveIden)]
enum BoardPostViews {
  Table,
  Id,
  PostId,
  UserId,
  CreatedAt,
}

/// board_post_reports
#[derive(DeriveIden)]
enum BoardPostReports {
  Table,
  Id,
  PostId,
  UserId,
  Reason,
  CreatedAt,
}

/// board_files
#[derive(DeriveIden)]
enum BoardFiles {
  Table,
  PostId,
  FileId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // board_categories
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardCategories::Table)
          .col(integer(BoardCategories::Id).auto_increment())
          .col(integer_null(BoardCategories::ParentId))
          .col(string(BoardCategories::Name))
          .col(string_null(BoardCategories::Description))
          .col(timestamp_with_time_zone(BoardCategories::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardCategories::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_categories__id").col(BoardCategories::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_categories__parent_id")
              .from(BoardCategories::Table, BoardCategories::ParentId)
              .to(BoardCategories::Table, BoardCategories::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_categories__parent_id")
          .table(BoardCategories::Table)
          .col(BoardCategories::ParentId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "board_categories").await?;

    // boards
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Boards::Table)
          .col(integer(Boards::Id).auto_increment())
          .col(integer_null(Boards::CategoryId))
          .col(string(Boards::Name))
          .col(string_null(Boards::Description))
          .col(boolean(Boards::CanUploadFile).default(false))
          .col(integer(Boards::MaxFileCount).default(0))
          .col(big_integer(Boards::MaxFileSize).default(0))
          .col(json_binary_null(Boards::AllowedFileTypes))
          .col(string(Boards::ReactionVisibility).default("private"))
          .col(string(Boards::ViewerVisibility).default("private"))
          .col(boolean(Boards::IsLogged).default(false))
          .col(boolean(Boards::IsEnabled).default(false))
          .col(timestamp_with_time_zone(Boards::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Boards::UpdatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone_null(Boards::DeletedAt))
          .primary_key(Index::create().name("pk_boards__id").col(Boards::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_boards__category_id")
              .from(Boards::Table, Boards::CategoryId)
              .to(BoardCategories::Table, BoardCategories::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_boards__category_id")
          .table(Boards::Table)
          .col(Boards::CategoryId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "boards").await?;

    // board_posts
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPosts::Table)
          .col(integer(BoardPosts::Id).auto_increment())
          .col(integer_null(BoardPosts::ParentId))
          .col(integer(BoardPosts::BoardId))
          .col(integer_null(BoardPosts::UserId))
          .col(string(BoardPosts::Kind).default("article"))
          .col(string(BoardPosts::Status).default("draft"))
          .col(integer_null(BoardPosts::Revision))
          .col(string(BoardPosts::Title))
          .col(string_null(BoardPosts::CompressionType))
          .col(string_null(BoardPosts::ContentType))
          .col(text_null(BoardPosts::Content))
          .col(text_null(BoardPosts::Text))
          .col(timestamp_with_time_zone_null(BoardPosts::PublishedAt))
          .col(timestamp_with_time_zone_null(BoardPosts::ScheduledAt))
          .col(integer(BoardPosts::Views).default(0))
          .col(json_binary_null(BoardPosts::Reactions))
          .col(ColumnDef::new(BoardPosts::Tags).array(ColumnType::Text).null())
          .col(timestamp_with_time_zone(BoardPosts::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardPosts::UpdatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone_null(BoardPosts::DeletedAt))
          .primary_key(Index::create().name("pk_board_posts__id").col(BoardPosts::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_posts__user_id")
              .from(BoardPosts::Table, BoardPosts::UserId)
              .to(Boards::Table, Boards::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_posts__board_id")
              .from(BoardPosts::Table, BoardPosts::BoardId)
              .to(Boards::Table, Boards::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_posts__parent_id")
              .from(BoardPosts::Table, BoardPosts::ParentId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__user_id")
          .table(BoardPosts::Table)
          .col(BoardPosts::UserId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__board_id")
          .table(BoardPosts::Table)
          .col(BoardPosts::BoardId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__parent_id")
          .table(BoardPosts::Table)
          .col(BoardPosts::ParentId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "board_posts").await?;

    // board_post_reactions
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostReactions::Table)
          .col(integer(BoardPostReactions::Id).auto_increment())
          .col(integer(BoardPostReactions::PostId))
          .col(integer_null(BoardPostReactions::UserId))
          .col(string(BoardPostReactions::Value))
          .col(timestamp_with_time_zone(BoardPostReactions::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_post_reactions__id").col(BoardPostReactions::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_reactions__post_id")
              .from(BoardPostReactions::Table, BoardPostReactions::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_post_reactions__post_id")
          .table(BoardPostReactions::Table)
          .col(BoardPostReactions::PostId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;

    // board_post_views
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostViews::Table)
          .col(integer(BoardPostViews::Id).auto_increment())
          .col(integer(BoardPostViews::PostId))
          .col(integer_null(BoardPostViews::UserId))
          .col(timestamp_with_time_zone(BoardPostViews::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_post_views__id").col(BoardPostViews::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_views__post_id")
              .from(BoardPostViews::Table, BoardPostViews::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_post_views__post_id")
          .table(BoardPostViews::Table)
          .col(BoardPostViews::PostId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;

    // board_post_reports
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostReports::Table)
          .col(integer(BoardPostReports::Id).auto_increment())
          .col(integer(BoardPostReports::PostId))
          .col(integer(BoardPostReports::UserId))
          .col(string(BoardPostReports::Reason))
          .col(timestamp_with_time_zone(BoardPostReports::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_post_reports__id").col(BoardPostReports::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_reports__post_id")
              .from(BoardPostReports::Table, BoardPostReports::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_post_reports__post_id")
          .table(BoardPostReports::Table)
          .col(BoardPostReports::PostId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;

    // board_files
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardFiles::Table)
          .col(integer_null(BoardFiles::PostId))
          .col(integer(BoardFiles::FileId))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_files__post_id")
              .from(BoardFiles::Table, BoardFiles::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_files__file_id")
              .from(BoardFiles::Table, BoardFiles::FileId)
              .to(Boards::Table, Boards::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_files__file_id")
          .table(BoardFiles::Table)
          .col(BoardFiles::FileId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_files__post_id")
          .table(BoardFiles::Table)
          .col(BoardFiles::PostId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "board_files").await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    detach_timestamp_auto_update(db, "board_categories").await?;
    detach_timestamp_auto_update(db, "boards").await?;
    detach_timestamp_auto_update(db, "board_posts").await?;
    detach_timestamp_auto_update(db, "board_files").await?;

    drop_table(manager, BoardFiles::Table).await?;
    drop_table(manager, BoardPostReports::Table).await?;
    drop_table(manager, BoardPostViews::Table).await?;
    drop_table(manager, BoardPostReactions::Table).await?;
    drop_table(manager, BoardPosts::Table).await?;
    drop_table(manager, Boards::Table).await?;
    drop_table(manager, BoardCategories::Table).await?;

    Ok(())
  }
}
