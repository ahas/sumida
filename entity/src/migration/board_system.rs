use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;

/// board_categories
#[derive(DeriveIden)]
enum BoardCategories {
  Table,
  Id,
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
  CreatedAt,
  UpdatedAt,
}

/// board_posts
#[derive(DeriveIden)]
enum BoardPosts {
  Table,
  Id,
  BoardId,
  UserId,
  Title,
  Content,
  CreatedAt,
  UpdatedAt,
}

/// board_comments
#[derive(DeriveIden)]
enum BoardComments {
  Table,
  Id,
  PostId,
  UserId,
  Content,
  CreatedAt,
  UpdatedAt,
}

/// board_tags
#[derive(DeriveIden)]
enum BoardTags {
  Table,
  Id,
  Name,
}

/// board_post_tags
#[derive(DeriveIden)]
enum BoardPostTags {
  Table,
  PostId,
  TagId,
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

/// board_post_attachments
#[derive(DeriveIden)]
enum BoardPostAttachments {
  Table,
  Id,
  PostId,
  FileUrl,
  FileType,
  UploadedAt,
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
pub struct CreateBoardSystem;

#[async_trait::async_trait]
impl MigrationTrait for CreateBoardSystem {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    // board_categories
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardCategories::Table)
          .col(ColumnDef::new(BoardCategories::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardCategories::Name).string().not_null())
          .col(ColumnDef::new(BoardCategories::Description).string().null())
          .col(
            ColumnDef::new(BoardCategories::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .col(
            ColumnDef::new(BoardCategories::UpdatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .primary_key(Index::create().name("pk_board_categories__id").col(BoardCategories::Id))
          .to_owned(),
      )
      .await?;

    // boards
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Boards::Table)
          .col(ColumnDef::new(Boards::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(Boards::CategoryId).integer().not_null())
          .col(ColumnDef::new(Boards::Name).string().not_null())
          .col(ColumnDef::new(Boards::Description).string().null())
          .col(
            ColumnDef::new(Boards::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .col(
            ColumnDef::new(Boards::UpdatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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

    // board_posts
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPosts::Table)
          .col(ColumnDef::new(BoardPosts::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardPosts::BoardId).integer().not_null())
          .col(ColumnDef::new(BoardPosts::UserId).integer().not_null())
          .col(ColumnDef::new(BoardPosts::Title).string().not_null())
          .col(ColumnDef::new(BoardPosts::Content).text().not_null())
          .col(
            ColumnDef::new(BoardPosts::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .col(
            ColumnDef::new(BoardPosts::UpdatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .primary_key(Index::create().name("pk_board_posts__id").col(BoardPosts::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_posts__board_id")
              .from(BoardPosts::Table, BoardPosts::BoardId)
              .to(Boards::Table, Boards::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    // board_comments
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardComments::Table)
          .col(ColumnDef::new(BoardComments::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardComments::PostId).integer().not_null())
          .col(ColumnDef::new(BoardComments::UserId).integer().not_null())
          .col(ColumnDef::new(BoardComments::Content).text().not_null())
          .col(
            ColumnDef::new(BoardComments::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .col(
            ColumnDef::new(BoardComments::UpdatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .primary_key(Index::create().name("pk_board_comments__id").col(BoardComments::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_comments__post_id")
              .from(BoardComments::Table, BoardComments::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    // board_tags
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardTags::Table)
          .col(ColumnDef::new(BoardTags::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardTags::Name).string().not_null())
          .primary_key(Index::create().name("pk_board_tags__id").col(BoardTags::Id))
          .to_owned(),
      )
      .await?;

    // board_post_tags
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostTags::Table)
          .col(ColumnDef::new(BoardPostTags::PostId).integer().not_null())
          .col(ColumnDef::new(BoardPostTags::TagId).integer().not_null())
          .primary_key(
            Index::create()
              .name("pk_board_post_tags__post_id__tag_id")
              .col(BoardPostTags::PostId)
              .col(BoardPostTags::TagId),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_tags__post_id")
              .from(BoardPostTags::Table, BoardPostTags::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_tags__tag_id")
              .from(BoardPostTags::Table, BoardPostTags::TagId)
              .to(BoardTags::Table, BoardTags::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    // board_post_likes
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostLikes::Table)
          .col(ColumnDef::new(BoardPostLikes::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardPostLikes::PostId).integer().not_null())
          .col(ColumnDef::new(BoardPostLikes::UserId).integer().not_null())
          .col(
            ColumnDef::new(BoardPostLikes::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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
          .col(ColumnDef::new(BoardCommentLikes::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardCommentLikes::CommentId).integer().not_null())
          .col(ColumnDef::new(BoardCommentLikes::UserId).integer().not_null())
          .col(
            ColumnDef::new(BoardCommentLikes::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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

    // board_post_attachments
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostAttachments::Table)
          .col(ColumnDef::new(BoardPostAttachments::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardPostAttachments::PostId).integer().not_null())
          .col(ColumnDef::new(BoardPostAttachments::FileUrl).string().not_null())
          .col(ColumnDef::new(BoardPostAttachments::FileType).string().not_null())
          .col(
            ColumnDef::new(BoardPostAttachments::UploadedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
          .primary_key(Index::create().name("pk_board_post_attachments__id").col(BoardPostAttachments::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_board_post_attachments__post_id")
              .from(BoardPostAttachments::Table, BoardPostAttachments::PostId)
              .to(BoardPosts::Table, BoardPosts::Id)
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
          .col(ColumnDef::new(BoardPostViews::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardPostViews::PostId).integer().not_null())
          .col(ColumnDef::new(BoardPostViews::UserId).integer().null())
          .col(ColumnDef::new(BoardPostViews::IpAddress).string().null())
          .col(
            ColumnDef::new(BoardPostViews::ViewedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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
          .col(ColumnDef::new(BoardPostReports::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardPostReports::PostId).integer().not_null())
          .col(ColumnDef::new(BoardPostReports::UserId).integer().not_null())
          .col(ColumnDef::new(BoardPostReports::Reason).string().not_null())
          .col(
            ColumnDef::new(BoardPostReports::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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
          .col(ColumnDef::new(BoardCommentReports::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardCommentReports::CommentId).integer().not_null())
          .col(ColumnDef::new(BoardCommentReports::UserId).integer().not_null())
          .col(ColumnDef::new(BoardCommentReports::Reason).string().not_null())
          .col(
            ColumnDef::new(BoardCommentReports::CreatedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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

    // board_post_histories
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(BoardPostHistories::Table)
          .col(ColumnDef::new(BoardPostHistories::Id).integer().not_null().auto_increment())
          .col(ColumnDef::new(BoardPostHistories::PostId).integer().not_null())
          .col(ColumnDef::new(BoardPostHistories::Content).text().not_null())
          .col(ColumnDef::new(BoardPostHistories::ModifiedBy).integer().not_null())
          .col(
            ColumnDef::new(BoardPostHistories::ModifiedAt)
              .timestamp_with_time_zone()
              .not_null()
              .default(Expr::current_timestamp()),
          )
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
    // drop in reverse order
    manager.drop_table(Table::drop().if_exists().table(BoardPostHistories::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardCommentReports::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostReports::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostViews::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostAttachments::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardCommentLikes::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostLikes::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPostTags::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardTags::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardComments::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardPosts::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Boards::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(BoardCategories::Table).to_owned()).await?;
    Ok(())
  }
}
