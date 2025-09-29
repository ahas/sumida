pub mod dto;

use crate::SumidaState;
use crate::service::ApiService;
use crate::util::pagination::Pagination;
use chrono::Utc;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::{
  ActiveModelBehavior, ActiveModelTrait, ColumnTrait, DeleteResult, EntityTrait, InsertResult,
  IntoActiveModel, IntoSimpleExpr, PaginatorTrait, QueryFilter, QuerySelect, Select, TransactionTrait,
  sea_query::{Asterisk, Expr, Func},
};

use self::dto::*;

use crate::util::ap::{self, ApCondition, ApGuard};

pub struct BoardFileService<S: SumidaState> {
  state: S,
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

    let db = self.state.db();
    let mut query = filter.apply(self.select(BoardFiles::find()));
    let count = query.clone().count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "filename" => Column::OriginalFilename.into_simple_expr(),
      "size" => Column::FileSize.into_simple_expr(),
      "mime_type" => Column::MimeType.into_simple_expr(),
      "status" => Column::UploadStatus.into_simple_expr(),
      "created_at" => Column::CreatedAt.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardFileListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardFile>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let model = self.select(BoardFiles::find_by_id(id)).one(&txn).await?;
    txn.commit().await?;

    Ok(model.map(Into::into))
  }

  pub async fn create(
    &self,
    body: CreateBoardFile,
  ) -> Result<InsertResult<entity::bs::board_files::ActiveModel>, SumidaError> {
    let db = self.state.db();
    let mut active_model: entity::bs::board_files::ActiveModel = body.into_active_model();

    // Set timestamps
    let now = Utc::now().fixed_offset();
    active_model.created_at = Set(now);
    active_model.updated_at = Set(now);

    // Set default values if not provided
    if active_model.is_temporary.is_not_set() {
      active_model.is_temporary = Set(false);
    }
    if active_model.upload_status.is_not_set() {
      active_model.upload_status = Set("pending".to_string());
    }

    let result = BoardFiles::insert(active_model).exec(db).await?;
    Ok(result)
  }

  pub async fn update(&self, id: i32, body: UpdateBoardFile) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;

    // Then check file-specific permission
    self.try_access("update", BoardFiles::find_by_id(id), &txn).await?;

    let mut active_model: entity::bs::board_files::ActiveModel = body.into_active_model();

    // Set updated timestamp
    active_model.updated_at = Set(Utc::now().fixed_offset());
    active_model.id = Set(id);

    active_model.update(&txn).await?;
    txn.commit().await?;
    Ok(())
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Then check file-specific permission
    self.try_access("delete", BoardFiles::find_by_id(id), &txn).await?;

    let result = BoardFiles::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn get_stats(&self, filter: BoardFileFilter) -> Result<BoardFileStats, SumidaError> {
    use entity::bs::board_files::Column;

    let db = self.state.db();
    let base_query = filter.apply(BoardFiles::find());

    // Total files and size
    let total_result = base_query
      .clone()
      .select_only()
      .expr_as(Func::count(Expr::col(Column::Id)), "total_files")
      .expr_as(Func::sum(Expr::col(Column::FileSize)), "total_size")
      .into_tuple::<(i64, Option<i64>)>()
      .one(db)
      .await?;

    let (total_files, total_size) = total_result.unwrap_or((0, None));
    let total_size = total_size.unwrap_or(0);

    // By MIME type
    let mime_type_stats = base_query
      .clone()
      .select_only()
      .column(Column::MimeType)
      .expr_as(Func::count(Expr::col(Column::Id)), "count")
      .expr_as(Func::sum(Expr::col(Column::FileSize)), "total_size")
      .group_by(Column::MimeType)
      .into_tuple::<(String, i64, Option<i64>)>()
      .all(db)
      .await?
      .into_iter()
      .map(|(mime_type, count, size)| FileMimeTypeStats {
        mime_type,
        count,
        total_size: size.unwrap_or(0),
      })
      .collect();

    // By status
    let status_stats = base_query
      .clone()
      .select_only()
      .column(Column::UploadStatus)
      .expr_as(Func::count(Expr::col(Column::Id)), "count")
      .group_by(Column::UploadStatus)
      .into_tuple::<(String, i64)>()
      .all(db)
      .await?
      .into_iter()
      .map(|(status, count)| FileStatusStats { status, count })
      .collect();

    Ok(BoardFileStats {
      total_files,
      total_size,
      by_mime_type: mime_type_stats,
      by_status: status_stats,
    })
  }

  pub async fn get_download_info(&self, id: i32) -> Result<Option<BoardFileDownload>, SumidaError> {
    let db = self.state.db();
    let model = BoardFiles::find_by_id(id).one(db).await?;

    Ok(model.map(|m| BoardFileDownload {
      content_type: m.mime_type,
      filename: m.original_filename,
      file_path: m.file_path,
    }))
  }

  pub async fn mark_as_completed(
    &self,
    id: i32,
    file_url: Option<String>,
    file_hash: Option<String>,
  ) -> Result<(), SumidaError> {
    let db = self.state.db();
    let mut active_model = <entity::bs::board_files::ActiveModel as ActiveModelBehavior>::new();

    active_model.id = Set(id);
    active_model.upload_status = Set("completed".to_string());
    active_model.updated_at = Set(Utc::now().fixed_offset());

    if let Some(url) = file_url {
      active_model.file_url = Set(Some(url));
    }
    if let Some(hash) = file_hash {
      active_model.file_hash = Set(Some(hash));
    }

    active_model.update(db).await?;
    Ok(())
  }

  pub async fn mark_as_failed(&self, id: i32) -> Result<(), SumidaError> {
    let db = self.state.db();
    let mut active_model = <entity::bs::board_files::ActiveModel as ActiveModelBehavior>::new();

    active_model.id = Set(id);
    active_model.upload_status = Set("failed".to_string());
    active_model.updated_at = Set(Utc::now().fixed_offset());

    active_model.update(db).await?;
    Ok(())
  }

  pub async fn cleanup_expired(&self) -> Result<u64, SumidaError> {
    use entity::bs::board_files::Column;

    let db = self.state.db();
    let now = Utc::now().fixed_offset();

    let result = BoardFiles::delete_many()
      .filter(Column::IsTemporary.eq(true))
      .filter(Column::ExpiresAt.lt(now))
      .exec(db)
      .await?;

    Ok(result.rows_affected)
  }

  pub async fn cleanup_failed(&self, older_than_hours: i64) -> Result<u64, SumidaError> {
    use chrono::Duration;
    use entity::bs::board_files::Column;

    let db = self.state.db();
    let cutoff = Utc::now() - Duration::hours(older_than_hours);

    let result = BoardFiles::delete_many()
      .filter(Column::UploadStatus.eq("failed"))
      .filter(Column::CreatedAt.lt(cutoff.fixed_offset()))
      .exec(db)
      .await?;

    Ok(result.rows_affected)
  }

  pub async fn get_file_info(
    &self,
    id: i32,
  ) -> Result<Option<entity::bs::board_files::Model>, SumidaError> {
    let db = self.state.db();
    let file = BoardFiles::find_by_id(id).one(db).await?;
    Ok(file)
  }

  pub async fn get_expired_files(&self) -> Result<Vec<entity::bs::board_files::Model>, SumidaError> {
    use entity::bs::board_files::Column;

    let db = self.state.db();
    let now = Utc::now().fixed_offset();
    let files = BoardFiles::find()
      .filter(Column::IsTemporary.eq(true))
      .filter(Column::ExpiresAt.lt(now))
      .all(db)
      .await?;
    Ok(files)
  }
}

impl<S: SumidaState> ApiService<ApCondition, BoardFiles> for BoardFileService<S> {
  fn default_query_with(query: Select<BoardFiles>) -> Select<BoardFiles> {
    query.select_only().expr(Expr::col((BoardFiles, Asterisk)))
  }

  fn ap(&self) -> ApCondition {
    use entity::bs::board_files::Column;

    ap::ns("board_file").who(&self.session).join(BoardFiles, Column::Id)
  }
}
