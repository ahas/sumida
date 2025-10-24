pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::{
  ColumnTrait,
  DeleteResult,
  EntityTrait,
  IntoSimpleExpr,
  JoinType,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  RelationTrait,
  Select,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

pub struct BoardFileService<S: SumidaState> {
  state:   S,
  #[allow(dead_code)]
  session: UserSession,
}

impl<S: SumidaState> BoardFileService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardFileFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardFileListItem>, u64), SumidaError> {
    use entity::bs::board_files::Column;
    use entity::fs::files::Column as FileColumn;

    let db = self.state.db();

    let mut query = filter.apply(
      BoardFiles::find()
        .join(JoinType::InnerJoin, entity::bs::board_files::Relation::File.def())
        .select_only()
        .column_as(Column::FileId, "id")
        .column_as(Column::PostId, "post_id")
        .column_as(FileColumn::Path, "path")
        .column_as(FileColumn::Uri, "uri")
        .column_as(FileColumn::Size, "size")
        .column_as(FileColumn::MimeType, "mime_type")
        .column_as(FileColumn::Status, "status")
        .column_as(FileColumn::ExpiresAt, "expires_at")
        .column_as(FileColumn::CreatedAt, "created_at")
        .column_as(FileColumn::UpdatedAt, "updated_at")
        .column_as(FileColumn::UpdatedAt, "deleted_at"),
    );

    let count = query.clone().count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "path" => FileColumn::Path.into_simple_expr(),
      "size" => FileColumn::Size.into_simple_expr(),
      "mime_type" => FileColumn::MimeType.into_simple_expr(),
      "status" => FileColumn::Status.into_simple_expr(),
      "created_at" => FileColumn::CreatedAt.into_simple_expr(),
      _ => FileColumn::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<BoardFileListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, post_id: i32, file_id: i32) -> Result<Option<GetBoardFile>, SumidaError> {
    use entity::bs::board_files::Column;
    use entity::fs::files::Column as FileColumn;

    let db = self.state.db();

    let result = BoardFiles::find()
      .filter(Column::PostId.eq(post_id))
      .filter(Column::FileId.eq(file_id))
      .join(JoinType::InnerJoin, entity::bs::board_files::Relation::File.def())
      .select_only()
      .column_as(Column::PostId, "post_id")
      .column_as(Column::FileId, "file_id")
      .column_as(FileColumn::Path, "path")
      .column_as(FileColumn::Uri, "uri")
      .column_as(FileColumn::Size, "size")
      .column_as(FileColumn::MimeType, "mime_type")
      .column_as(FileColumn::Hash, "hash")
      .column_as(FileColumn::Status, "status")
      .column_as(FileColumn::ExpiresAt, "expires_at")
      .column_as(FileColumn::CreatedAt, "created_at")
      .column_as(FileColumn::UpdatedAt, "updated_at")
      .into_model::<GetBoardFile>()
      .one(db)
      .await?;

    Ok(result)
  }

  pub async fn attach_file(&self, post_id: i32, file_id: i32) -> Result<(), SumidaError> {
    let db = self.state.db();
    let active_model = entity::bs::board_files::ActiveModel {
      post_id: Set(post_id),
      file_id: Set(file_id),
    };

    BoardFiles::insert(active_model).exec(db).await?;
    Ok(())
  }

  pub async fn detach_file(&self, post_id: i32, file_id: i32) -> Result<DeleteResult, SumidaError> {
    use entity::bs::board_files::Column;

    let db = self.state.db();

    let result = BoardFiles::delete_many()
      .filter(Column::PostId.eq(post_id))
      .filter(Column::FileId.eq(file_id))
      .exec(db)
      .await?;

    Ok(result)
  }

  pub async fn get_files_for_post(&self, post_id: i32) -> Result<Vec<BoardFileListItem>, SumidaError> {
    use entity::bs::board_files::Column;
    use entity::fs::files::Column as FileColumn;

    let db = self.state.db();

    let items = BoardFiles::find()
      .filter(Column::PostId.eq(post_id))
      .join(JoinType::InnerJoin, entity::bs::board_files::Relation::File.def())
      .select_only()
      .column_as(Column::FileId, "id")
      .column_as(Column::PostId, "post_id")
      .column_as(FileColumn::Path, "path")
      .column_as(FileColumn::Uri, "uri")
      .column_as(FileColumn::Size, "size")
      .column_as(FileColumn::MimeType, "mime_type")
      .column_as(FileColumn::Status, "status")
      .column_as(FileColumn::ExpiresAt, "expires_at")
      .column_as(FileColumn::CreatedAt, "created_at")
      .column_as(FileColumn::UpdatedAt, "updated_at")
      .column_as(FileColumn::DeletedAt, "deleted_at")
      .into_model::<BoardFileListItem>()
      .all(db)
      .await?;

    Ok(items)
  }

  pub async fn get_posts_for_file(&self, file_id: i32) -> Result<Vec<i32>, SumidaError> {
    use entity::bs::board_files::Column;

    let db = self.state.db();

    let post_ids = BoardFiles::find()
      .filter(Column::FileId.eq(file_id))
      .select_only()
      .column(Column::PostId)
      .into_tuple::<i32>()
      .all(db)
      .await?;

    Ok(post_ids)
  }
}

impl<S: SumidaState> ApiService<ApCondition, BoardFiles> for BoardFileService<S> {
  fn default_query_with(query: Select<BoardFiles>) -> Select<BoardFiles> {
    query
  }

  fn ap(&self) -> ApCondition {
    use entity::bs::board_files::Column;

    ap::ns("board_file").who(&self.session).join(BoardFiles, Column::PostId)
  }
}
