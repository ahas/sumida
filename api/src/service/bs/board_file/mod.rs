pub mod dto;
pub mod util;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
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
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::service::fs::dto::{DeleteFile, FileUploaded};
use crate::service::fs::file::FileService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::file_validation::FileValidator;
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

    let mut query = filter.apply(util::with_file_columns(
      BoardFiles::find()
        .join(JoinType::InnerJoin, entity::bs::board_files::Relation::File.def())
        .select_only()
        .column_as(Column::FileId, "id")
        .column_as(Column::PostId, "post_id"),
    ));

    let count = query.clone().count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "dir" => FileColumn::Dir.into_simple_expr(),
      "name" => FileColumn::Name.into_simple_expr(),
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

    let result = util::with_file_columns(
      BoardFiles::find()
        .filter(Column::PostId.eq(post_id))
        .filter(Column::FileId.eq(file_id))
        .join(JoinType::InnerJoin, entity::bs::board_files::Relation::File.def())
        .select_only()
        .column_as(Column::PostId, "post_id")
        .column_as(Column::FileId, "file_id")
        .column_as(FileColumn::Hash, "hash"),
    )
    .into_model::<GetBoardFile>()
    .one(db)
    .await?;

    Ok(result)
  }

  pub async fn upload(&self, data: UploadBoardFile) -> Result<FileUploaded, SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(board) = Boards::find_by_id(data.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".to_string(), data.board_id.to_string()));
    };

    FileValidator::from_board_id(board.id, &txn)
      .await?
      .validate_upload_enabled()?
      .validate_extension(data.file.get_extension().as_deref())?
      .validate_size(data.file.bytes.len())?
      .validate_mime_type(&data.file.mime_type)?;

    let result = FileService::new(self.state.clone(), self.session.clone()).upload(data.into()).await?;

    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, data: DeleteBoardFile) -> Result<DeleteResult, SumidaError> {
    let db = self.state.db();

    let result = BoardFiles::delete_many()
      .filter(BoardFilesColumn::PostId.eq(data.post_id))
      .filter(BoardFilesColumn::FileId.eq(data.file_id))
      .exec(db)
      .await?;

    FileService::new(self.state.clone(), self.session.clone())
      .delete(data.file_id, DeleteFile { hard: data.hard })
      .await?;

    Ok(result)
  }

  pub async fn get_files_for_post(&self, post_id: i32) -> Result<Vec<BoardFileListItem>, SumidaError> {
    use entity::bs::board_files::Column;

    let db = self.state.db();

    let items = util::with_file_columns(
      BoardFiles::find()
        .filter(Column::PostId.eq(post_id))
        .join(JoinType::InnerJoin, entity::bs::board_files::Relation::File.def())
        .select_only()
        .column_as(Column::FileId, "id")
        .column_as(Column::PostId, "post_id"),
    )
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
