use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

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
  IsSystem,
  SystemCategory,
  MaxFileSize,
  AllowedFileTypes,
  MaxFilesPerPost,
  EnableFileUpload,
  EnablePostHistory,
  MaxHistoryCount,
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
  UserId,
  Title,
  Content,
  Text,
  Status,
  PublishedAt,
  ScheduledAt,
  AutoSaveContent,
  LastAutoSaveAt,
  CreatedAt,
  UpdatedAt,
}

/// board_comments
#[derive(DeriveIden)]
enum BoardComments {
  Table,
  Id,
  ParentId,
  PostId,
  UserId,
  Content,
  CreatedAt,
  UpdatedAt,
}

/// board_post_likes
#[derive(DeriveIden)]
enum BoardPostLikes {
  Table,
  Id,
  PostId,
  UserId,
  CreatedAt,
}

/// board_comment_likes
#[derive(DeriveIden)]
enum BoardCommentLikes {
  Table,
  Id,
  CommentId,
  UserId,
  CreatedAt,
}


/// board_post_views
#[derive(DeriveIden)]
enum BoardPostViews {
  Table,
  Id,
  PostId,
  UserId,
  IpAddress,
  ViewedAt,
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

/// board_comment_reports
#[derive(DeriveIden)]
enum BoardCommentReports {
  Table,
  Id,
  CommentId,
  UserId,
  Reason,
  CreatedAt,
}

/// board_files
#[derive(DeriveIden)]
enum BoardFiles {
  Table,
  Id,
  BoardId,
  PostId,
  CommentId,
  UserId,
  OriginalFilename,
  FilePath,
  FileUrl,
  FileSize,
  MimeType,
  FileHash,
  UploadStatus,
  IsTemporary,
  ExpiresAt,
  CreatedAt,
  UpdatedAt,
}

/// board_post_histories
#[derive(DeriveIden)]
enum BoardPostHistories {
  Table,
  Id,
  PostId,
  Content,
  ModifiedBy,
  ModifiedAt,
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
    db.execute_unprepared("SELECT manage_updated_at('board_categories');").await?;

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
          .col(boolean(Boards::IsSystem).default(false))
          .col(string_null(Boards::SystemCategory))
          .col(big_integer(Boards::MaxFileSize).default(10485760)) // 10MB
          .col(json(Boards::AllowedFileTypes).default(r#"["jpg","jpeg","png","gif","pdf","doc","docx","txt"]"#))
          .col(integer(Boards::MaxFilesPerPost).default(5))
          .col(boolean(Boards::EnableFileUpload).default(true))
          .col(boolean(Boards::EnablePostHistory).default(false))
          .col(integer(Boards::MaxHistoryCount).default(10))
          .col(timestamp_with_time_zone(Boards::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Boards::UpdatedAt).default(Expr::current_timestamp()))
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
    db.execute_unprepared("SELECT manage_updated_at('boards');").await?;

    // board_posts
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPosts::Table)
          .col(integer(BoardPosts::Id).auto_increment())
          .col(integer_null(BoardPosts::ParentId))
          .col(integer(BoardPosts::BoardId))
          .col(integer(BoardPosts::UserId))
          .col(string(BoardPosts::Title))
          .col(json_binary_null(BoardPosts::Content))
          .col(text_null(BoardPosts::Text))
          .col(string(BoardPosts::Status).default("draft"))
          .col(timestamp_with_time_zone_null(BoardPosts::PublishedAt))
          .col(timestamp_with_time_zone_null(BoardPosts::ScheduledAt))
          .col(text_null(BoardPosts::AutoSaveContent))
          .col(timestamp_with_time_zone_null(BoardPosts::LastAutoSaveAt))
          .col(timestamp_with_time_zone(BoardPosts::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardPosts::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_posts__id").col(BoardPosts::Id))
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
    db.execute_unprepared("SELECT manage_updated_at('board_posts');").await?;

    // board_comments
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardComments::Table)
          .col(integer(BoardComments::Id).auto_increment())
          .col(integer_null(BoardComments::ParentId))
          .col(integer(BoardComments::PostId))
          .col(integer(BoardComments::UserId))
          .col(text(BoardComments::Content))
          .col(timestamp_with_time_zone(BoardComments::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardComments::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_comments__id").col(BoardComments::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_comments__post_id")
              .from(BoardComments::Table, BoardComments::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_comments__parent_id")
              .from(BoardComments::Table, BoardComments::ParentId)
              .to(BoardComments::Table, BoardComments::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    db.execute_unprepared("SELECT manage_updated_at('board_comments');").await?;

    // board_post_likes
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostLikes::Table)
          .col(integer(BoardPostLikes::Id).auto_increment())
          .col(integer(BoardPostLikes::PostId))
          .col(integer(BoardPostLikes::UserId))
          .col(timestamp_with_time_zone(BoardPostLikes::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_post_likes__id").col(BoardPostLikes::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_likes__post_id")
              .from(BoardPostLikes::Table, BoardPostLikes::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    // board_comment_likes
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardCommentLikes::Table)
          .col(integer(BoardCommentLikes::Id).auto_increment())
          .col(integer(BoardCommentLikes::CommentId))
          .col(integer(BoardCommentLikes::UserId))
          .col(timestamp_with_time_zone(BoardCommentLikes::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_comment_likes__id").col(BoardCommentLikes::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_comment_likes__comment_id")
              .from(BoardCommentLikes::Table, BoardCommentLikes::CommentId)
              .to(BoardComments::Table, BoardComments::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
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
          .col(string_null(BoardPostViews::IpAddress))
          .col(timestamp_with_time_zone(BoardPostViews::ViewedAt).default(Expr::current_timestamp()))
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

    // board_comment_reports
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardCommentReports::Table)
          .col(integer(BoardCommentReports::Id).auto_increment())
          .col(integer(BoardCommentReports::CommentId))
          .col(integer(BoardCommentReports::UserId))
          .col(string(BoardCommentReports::Reason))
          .col(timestamp_with_time_zone(BoardCommentReports::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_comment_reports__id").col(BoardCommentReports::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_comment_reports__comment_id")
              .from(BoardCommentReports::Table, BoardCommentReports::CommentId)
              .to(BoardComments::Table, BoardComments::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    // board_files
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardFiles::Table)
          .col(integer(BoardFiles::Id).auto_increment())
          .col(integer_null(BoardFiles::BoardId))
          .col(integer_null(BoardFiles::PostId))
          .col(integer_null(BoardFiles::CommentId))
          .col(integer_null(BoardFiles::UserId))
          .col(string(BoardFiles::OriginalFilename))
          .col(string(BoardFiles::FilePath))
          .col(string_null(BoardFiles::FileUrl))
          .col(big_integer(BoardFiles::FileSize))
          .col(string(BoardFiles::MimeType))
          .col(string_null(BoardFiles::FileHash))
          .col(string(BoardFiles::UploadStatus).default("pending"))
          .col(boolean(BoardFiles::IsTemporary).default(false))
          .col(timestamp_with_time_zone_null(BoardFiles::ExpiresAt))
          .col(timestamp_with_time_zone(BoardFiles::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(BoardFiles::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_files__id").col(BoardFiles::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_files__board_id")
              .from(BoardFiles::Table, BoardFiles::BoardId)
              .to(Boards::Table, Boards::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
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
              .name("fk_board_files__comment_id")
              .from(BoardFiles::Table, BoardFiles::CommentId)
              .to(BoardComments::Table, BoardComments::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    db.execute_unprepared("SELECT manage_updated_at('board_files');").await?;

    // board_post_histories
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostHistories::Table)
          .col(integer(BoardPostHistories::Id).auto_increment())
          .col(integer(BoardPostHistories::PostId))
          .col(text(BoardPostHistories::Content))
          .col(integer(BoardPostHistories::ModifiedBy))
          .col(timestamp_with_time_zone(BoardPostHistories::ModifiedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_board_post_histories__id").col(BoardPostHistories::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_histories__post_id")
              .from(BoardPostHistories::Table, BoardPostHistories::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    // Tables
    manager.drop_table(Table::drop().if_exists().table(BoardPostHistories::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardFiles::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardCommentReports::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostReports::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostViews::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardCommentLikes::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostLikes::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardComments::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPosts::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Boards::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardCategories::Table).to_owned()).await?;
    Ok(())
  }
}
