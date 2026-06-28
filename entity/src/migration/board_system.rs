use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// board_groups
#[derive(DeriveIden)]
enum BoardGroups {
  Table,
  Id,
  ParentId,
  Name,
  Description,
  Order,
  CreatedAt,
  UpdatedAt,
}

/// boards
#[derive(DeriveIden)]
enum Boards {
  Table,
  Id,
  GroupId,
  Name,
  Description,
  Order,
  CanUploadFile,
  MaxFileSize,
  MaxFileCount,
  AllowedFileTypes,
  ReactionVisibility,
  ViewerVisibility,
  ContentVisibility,
  IsVersioned,
  IsEnabled,
  CreatedAt,
  UpdatedAt,
  DeletedAt,
}

#[derive(DeriveIden)]
enum BoardTopics {
  Table,
  Id,
  Name,
  Description,
  BoardId,
  Order,
  CreatedAt,
  UpdatedAt,
}

/// board_posts
#[derive(DeriveIden)]
enum BoardPosts {
  Table,
  Id,
  ParentId,
  BoardId,
  TopicId,
  UserId,
  Kind,
  Status,
  Revision,
  Title,
  Password,
  CompressionType,
  ContentType,
  Content,
  ContentVisibility,
  Text,
  TextTsv,
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
  FileUuid,
}

// files
#[derive(DeriveIden)]
enum Files {
  Table,
  Uuid,
}

// users
#[derive(DeriveIden)]
enum Users {
  Table,
  Id,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // board_groups
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardGroups::Table)
          .col(integer(BoardGroups::Id).auto_increment())
          .col(integer_null(BoardGroups::ParentId))
          .col(string(BoardGroups::Name))
          .col(string_null(BoardGroups::Description))
          .col(integer(BoardGroups::Order).default(0))
          .col(timestamp_with_time_zone(BoardGroups::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardGroups::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_groups__id").col(BoardGroups::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_groups__parent_id")
              .from(BoardGroups::Table, BoardGroups::ParentId)
              .to(BoardGroups::Table, BoardGroups::Id)
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
          .name("idx_board_groups__parent_id")
          .table(BoardGroups::Table)
          .col(BoardGroups::ParentId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "board_groups").await?;

    // boards
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Boards::Table)
          .col(integer(Boards::Id).auto_increment())
          .col(integer_null(Boards::GroupId))
          .col(string(Boards::Name))
          .col(string_null(Boards::Description))
          .col(integer(Boards::Order).default(0))
          .col(boolean(Boards::CanUploadFile).default(false))
          .col(integer(Boards::MaxFileCount).default(0))
          .col(big_integer(Boards::MaxFileSize).default(0))
          .col(json_binary_null(Boards::AllowedFileTypes))
          .col(string(Boards::ReactionVisibility).default("private"))
          .col(string(Boards::ViewerVisibility).default("private"))
          .col(string(Boards::ContentVisibility).default("private"))
          .col(boolean(Boards::IsVersioned).default(false))
          .col(boolean(Boards::IsEnabled).default(false))
          .col(timestamp_with_time_zone(Boards::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Boards::UpdatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone_null(Boards::DeletedAt))
          .primary_key(Index::create().name("pk_boards__id").col(Boards::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_boards__group_id")
              .from(Boards::Table, Boards::GroupId)
              .to(BoardGroups::Table, BoardGroups::Id)
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
          .name("idx_boards__group_id")
          .table(Boards::Table)
          .col(Boards::GroupId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "boards").await?;

    // board_topics
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardTopics::Table)
          .col(integer(BoardTopics::Id).auto_increment())
          .col(string(BoardTopics::Name))
          .col(string_null(BoardTopics::Description))
          .col(integer(BoardTopics::BoardId))
          .col(integer(BoardTopics::Order).default(0))
          .col(timestamp_with_time_zone(BoardTopics::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardTopics::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_topics__id").col(Boards::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_topics__board_id")
              .from(BoardTopics::Table, BoardTopics::BoardId)
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
          .table(BoardTopics::Table)
          .name("idx_board_topics__board_id")
          .col(BoardTopics::BoardId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;

    // board_posts
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPosts::Table)
          .col(integer(BoardPosts::Id).auto_increment())
          .col(integer_null(BoardPosts::ParentId))
          .col(integer(BoardPosts::BoardId))
          .col(integer_null(BoardPosts::TopicId))
          .col(integer_null(BoardPosts::UserId))
          .col(string(BoardPosts::Kind).default("article"))
          .col(string(BoardPosts::Status).default("draft"))
          .col(integer_null(BoardPosts::Revision))
          .col(string(BoardPosts::Title))
          .col(string_null(BoardPosts::Password))
          .col(string_null(BoardPosts::CompressionType))
          .col(string(BoardPosts::ContentType))
          .col(text_null(BoardPosts::Content))
          .col(string(BoardPosts::ContentVisibility).default("private"))
          .col(text_null(BoardPosts::Text))
          .col(ColumnDef::new(BoardPosts::TextTsv).custom("tsvector").null())
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
              .to(Users::Table, Users::Id)
              // Preserve posts when the author account is removed (user_id is nullable).
              .on_delete(SetNull)
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
              .name("fk_board_posts__topic_id")
              .from(BoardPosts::Table, BoardPosts::TopicId)
              .to(BoardTopics::Table, BoardTopics::Id)
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
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__content_type")
          .table(BoardPosts::Table)
          .col(BoardPosts::ContentType)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__topic_id")
          .table(BoardPosts::Table)
          .col(BoardPosts::TopicId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__status")
          .table(BoardPosts::Table)
          .col(BoardPosts::Status)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_posts__text")
          .table(BoardPosts::Table)
          .col(BoardPosts::TextTsv)
          .index_type(IndexType::FullText)
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
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_post_reactions__user_id")
          .table(BoardPostReactions::Table)
          .col(BoardPostReactions::UserId)
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
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_board_post_views__user_id")
          .table(BoardPostViews::Table)
          .col(BoardPostViews::UserId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    // One view marker per (post, user): makes "view once" race-safe (anonymous user_id NULLs stay distinct).
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .unique()
          .name("uq_board_post_views__post_user")
          .table(BoardPostViews::Table)
          .col(BoardPostViews::PostId)
          .col(BoardPostViews::UserId)
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
          .col(uuid(BoardFiles::FileUuid))
          .col(integer_null(BoardFiles::PostId))
          // file_uuid is the sole PK: a file links to at most one post (1:1). post_id is nullable
          // so a row can represent a *pending claim* (uploaded, not yet attached to a post).
          .primary_key(Index::create().name("pk_board_files__file_uuid").col(BoardFiles::FileUuid))
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
              .name("fk_board_files__file_uuid")
              .from(BoardFiles::Table, BoardFiles::FileUuid)
              .to(Files::Table, Files::Uuid)
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
          .name("idx_board_files__file_uuid")
          .table(BoardFiles::Table)
          .col(BoardFiles::FileUuid)
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
    // NOTE: board_files has no created_at/updated_at columns, so no timestamp auto-update trigger.

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    detach_timestamp_auto_update(db, "board_groups").await?;
    detach_timestamp_auto_update(db, "boards").await?;
    detach_timestamp_auto_update(db, "board_posts").await?;

    drop_table(manager, BoardFiles::Table).await?;
    drop_table(manager, BoardPostReports::Table).await?;
    drop_table(manager, BoardPostViews::Table).await?;
    drop_table(manager, BoardPostReactions::Table).await?;
    drop_table(manager, BoardPosts::Table).await?;
    drop_table(manager, BoardTopics::Table).await?;
    drop_table(manager, Boards::Table).await?;
    drop_table(manager, BoardGroups::Table).await?;

    Ok(())
  }
}
