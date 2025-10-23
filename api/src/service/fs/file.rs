use std::path::Path as StdPath;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::NotSet;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, Asterisk, Expr, Func};
use sea_orm::{
  DeleteResult,
  InsertResult,
  IntoActiveModel,
  IntoActiveValue,
  IntoSimpleExpr,
  JoinType,
  QuerySelect,
  Set,
  TransactionTrait,
};
use uuid::Uuid;

use super::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;
use crate::util::storage::{StaticFile, Storage, SumidaStorage};

pub struct FileService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> FileService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    FileService { state, session }
  }

  pub async fn list(
    &self,
    filter: FileFilter,
    pagination: Pagination,
  ) -> Result<(Vec<FileListItem>, u64), SumidaError> {
    use entity::fs::files::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(Files::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "user" => Column::UserId.into_simple_expr(),
      "path" => Column::Path.into_simple_expr(),
      "uri" => Column::Uri.into_simple_expr(),
      "status" => Column::Status.into_simple_expr(),
      "mimeType" => Column::MimeType.into_simple_expr(),
      "isTemporary" => Column::IsTemporary.into_simple_expr(),
      _ => Column::Id.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<FileListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetFile>, SumidaError> {
    Ok(self.select(Files::find_by_id(id)).into_model::<GetFile>().one(self.state.db()).await?)
  }

  pub async fn create(
    &self,
    file_data: UploadFileData,
    metadata: CreateFile,
  ) -> Result<InsertResult<entity::fs::files::ActiveModel>, SumidaError> {
    let file_extension = StdPath::new(&file_data.original_name)
      .extension()
      .and_then(|ext| ext.to_str())
      .unwrap_or("");
    let unique_filename = format!("{}.{}", Uuid::new_v4(), file_extension);
    let path = StdPath::new(&metadata.directory)
      .join(StdPath::new(&unique_filename))
      .display()
      .to_string();
    let static_file = StaticFile {
      path:          path.clone(),
      name:          unique_filename,
      mime_type:     file_data.mime_type.clone(),
      bytes:         file_data.bytes.clone(),
      original_name: file_data.original_name.clone(),
    };
    let storage = SumidaStorage::new();
    storage.store(static_file).await?;
    let file_url = storage.get_uri(&path);

    Ok(
      Files::insert(entity::fs::files::ActiveModel {
        id: NotSet,
        uri: Set(Some(file_url)),
        path: Set(path),
        original_name: Set(file_data.original_name.clone()),
        size: Set(file_data.bytes.len() as i64),
        mime_type: Set(file_data.mime_type.clone()),
        status: Set("uploaded".to_string()),
        user_id: Set(Some(self.session.profile.id)),
        is_temporary: metadata.is_temporary.into_active_value(),
        expires_at: metadata.expires_at.into_active_value(),
        hash: Set(Some(file_data.hash())),
        ..Default::default()
      })
      .exec(self.state.db())
      .await?,
    )
  }

  pub async fn update(&self, id: i32, data: UpdateFile) -> Result<entity::fs::files::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", Files::find_by_id(id), &txn).await?;

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Files::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", Files::find_by_id(id), &txn).await?;

    let result = Files::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn get_download_info(&self, id: i32) -> Result<Option<super::dto::FileDownload>, SumidaError> {
    let db = self.state.db();
    let model = Files::find_by_id(id).one(db).await?;

    Ok(model.map(|m| super::dto::FileDownload {
      content_type: m.mime_type,
      name:         m.original_name,
      path:         m.path,
    }))
  }

  pub async fn mark_as_completed(
    &self,
    id: i32,
    file_url: Option<String>,
    file_hash: Option<String>,
  ) -> Result<(), SumidaError> {
    let mut model = entity::fs::files::ActiveModel::new();

    model.id = Set(id);
    model.status = Set("completed".to_string());

    if let Some(url) = file_url {
      model.uri = Set(Some(url));
    }
    if let Some(hash) = file_hash {
      model.hash = Set(Some(hash));
    }

    Files::update(model).exec(self.state.db()).await?;
    Ok(())
  }

  pub async fn mark_as_failed(&self, id: i32) -> Result<(), SumidaError> {
    let mut model = entity::fs::files::ActiveModel::new();

    model.id = Set(id);
    model.status = Set("failed".to_string());

    Files::update(model).exec(self.state.db()).await?;
    Ok(())
  }

  pub async fn get_stats(
    &self,
    filter: super::dto::FileFilter,
  ) -> Result<super::dto::FileStats, SumidaError> {
    use entity::fs::files::Column;

    let db = self.state.db();
    let base_query = filter.apply(Files::find());

    let total_result = base_query
      .clone()
      .select_only()
      .expr_as(Func::count(Expr::col(Column::Id)), "total_files")
      .expr_as(Func::sum(Expr::col(Column::Size)), "total_size")
      .into_tuple::<(i64, Option<i64>)>()
      .one(db)
      .await?;

    let (total_files, total_size) = total_result.unwrap_or((0, None));
    let total_size = total_size.unwrap_or(0);

    let mime_type_stats = base_query
      .clone()
      .select_only()
      .column(Column::MimeType)
      .expr_as(Func::count(Expr::col(Column::Id)), "count")
      .expr_as(Func::sum(Expr::col(Column::Size)), "total_size")
      .group_by(Column::MimeType)
      .into_tuple::<(String, i64, Option<i64>)>()
      .all(db)
      .await?
      .into_iter()
      .map(|(mime_type, count, size)| super::dto::FileMimeTypeStats {
        mime_type,
        count,
        total_size: size.unwrap_or(0),
      })
      .collect();

    let status_stats = base_query
      .clone()
      .select_only()
      .column(Column::Status)
      .expr_as(Func::count(Expr::col(Column::Id)), "count")
      .group_by(Column::Status)
      .into_tuple::<(String, i64)>()
      .all(db)
      .await?
      .into_iter()
      .map(|(status, count)| super::dto::FileStatusStats { status, count })
      .collect();

    Ok(super::dto::FileStats {
      total_files,
      total_size,
      by_mime_type: mime_type_stats,
      by_status: status_stats,
    })
  }

  pub async fn cleanup_expired(&self) -> Result<u64, SumidaError> {
    use entity::fs::files::Column;

    let db = self.state.db();
    let now = chrono::Utc::now().fixed_offset();

    let result = Files::delete_many()
      .filter(Column::IsTemporary.eq(true))
      .filter(Column::ExpiresAt.lt(now))
      .exec(db)
      .await?;

    Ok(result.rows_affected)
  }

  pub async fn cleanup_failed(&self, older_than_hours: i64) -> Result<u64, SumidaError> {
    use chrono::Duration;
    use entity::fs::files::Column;

    let db = self.state.db();
    let cutoff = chrono::Utc::now() - Duration::hours(older_than_hours);

    let result = Files::delete_many()
      .filter(Column::Status.eq("failed"))
      .filter(Column::CreatedAt.lt(cutoff.fixed_offset()))
      .exec(db)
      .await?;

    Ok(result.rows_affected)
  }
}

impl<S: SumidaState> ApiService<ApCondition, Files> for FileService<S> {
  fn default_query_with(query: Select<Files>) -> Select<Files> {
    let user_alias = Alias::new("user");
    let col_id = Expr::col((user_alias.clone(), "id"));
    let col_username = Expr::col((user_alias.clone(), "username"));
    let col_full_name = Expr::col((user_alias.clone(), "full_name"));

    query
      .expr(Expr::col((entity::fs::files::Entity, Asterisk)))
      .expr_as(col_id, "user_id")
      .expr_as(col_username.clone(), "user_username")
      .expr_as(col_full_name.clone(), "user_full_name")
      .join_as(JoinType::LeftJoin, entity::fs::files::Relation::User.def(), user_alias.clone())
  }

  fn ap(&self) -> ApCondition {
    use entity::fs::files::Column;
    ap::ns("file").who(&self.session).join(Files, Column::Id)
  }

  fn select(&self, query: sea_orm::Select<Files>) -> sea_orm::Select<Files> {
    use entity::fs::files::Column;
    self
      .ap()
      .expr(Expr::col((Files, Column::Id)).cast_as("varchar"))
      .action("read")
      .apply(Self::default_query_with(query))
  }
}
