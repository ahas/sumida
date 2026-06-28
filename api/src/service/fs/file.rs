use axum::body::Body;
use axum::http::Response;
use entity::fs::files::FileStatus;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::{NotSet, Unchanged};
use sea_orm::prelude::*;
use sea_orm::sea_query::{Asterisk, Expr, Func};
use sea_orm::{DeleteResult, IntoActiveValue, IntoSimpleExpr, QuerySelect, Set, TransactionTrait};

use super::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::file_validation::{FileValidationConfig, FileValidator};
use crate::util::pagination::Pagination;
use crate::util::storage::{Storage, SumidaStorage};

pub struct FileService<S: SumidaState> {
  state: S,
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
    let count = query.clone().select_only().column(Column::Uuid).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "user" => Column::UserId.into_simple_expr(),
      "dir" => Column::Dir.into_simple_expr(),
      "name" => Column::Name.into_simple_expr(),
      "uri" => Column::Uri.into_simple_expr(),
      "status" => Column::Status.into_simple_expr(),
      "mimeType" => Column::MimeType.into_simple_expr(),
      _ => Column::Uuid.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items: Vec<FileListItem> = query.into_partial_model().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, uuid: Uuid) -> Result<Option<GetFile>, SumidaError> {
    Ok(
      self
        .select(Files::find_by_id(uuid))
        .into_partial_model::<GetFile>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn upload(&self, data: UploadFile) -> Result<FileUploaded, SumidaError> {
    let db = self.state.db();

    // General-purpose upload validation: size/extension/MIME checks based on the global storage.upload limits.
    // (Unlike board uploads, there is no per-board policy, so the global limits apply. Even if the limits are empty,
    //  validate_mime_type always blocks dangerous MIME types (executables/scripts).)
    let upload_limits = &infra::settings::get().storage.upload;
    FileValidator::new(FileValidationConfig {
      can_upload_file: true,
      max_file_size: upload_limits.max_file_size,
      max_file_count: 0,
      allowed_file_types: upload_limits.allowed_file_types.clone(),
    })
    .validate_size(data.file.bytes.len())?
    .validate_extension(data.file.get_extension().as_deref())?
    .validate_mime_type(&data.file.mime_type)?;

    let file_dir = data.options.dir.clone();
    let file_hash = data.file.hash();
    let file_tags = data.options.tags.clone();
    let file_expires_at = data.options.expires_at.clone();
    let file_original_name = data.file.original_name.clone();

    let static_file = data.into_static_file();
    let storage = SumidaStorage::new();

    let file_uuid = Files::insert(entity::fs::files::ActiveModel {
      uuid: NotSet,
      uri: Set(Some(storage.get_uri(&static_file.get_path()))),
      dir: Set(file_dir),
      name: Set(static_file.name.clone()),
      original_name: Set(file_original_name.clone()),
      size: Set(static_file.bytes.len() as i64),
      mime_type: Set(static_file.mime_type.clone()),
      status: Set(FileStatus::Pending),
      user_id: Set(Some(self.session.profile.id)),
      expires_at: file_expires_at.into_active_value(),
      hash: Set(Some(file_hash)),
      tags: Set(file_tags),
      ..Default::default()
    })
    .exec(db)
    .await?
    .last_insert_id;

    let result = storage.store(static_file).await;

    Files::update(entity::fs::files::ActiveModel {
      uuid: Unchanged(file_uuid),
      status: Set(if result.is_ok() { FileStatus::Temporary } else { FileStatus::Error }),
      ..Default::default()
    })
    .exec(db)
    .await?;

    result?;

    // Grant the uploader read access to their own file so they can preview/serve it
    // (e.g. a tiptap inline image while the post is still being composed).
    ap::edit()
      .namespace("file")
      .object(file_uuid)
      .action(vec!["read".to_string()])
      .user(ap::ApUser::User(self.session.profile.id))
      .effect("allow")
      .create(db)
      .await?;

    Ok(FileUploaded { uuid: file_uuid })
  }

  pub async fn download(&self, uuid: Uuid) -> Result<Response<Body>, SumidaError> {
    let Some(file) = self.select(Files::find_by_id(uuid)).one(self.state.db()).await? else {
      return Err(SumidaError::NoData("File".to_string(), uuid.to_string()));
    };
    let storage = SumidaStorage::new();
    let response = storage.download(file.into(), true).await?;

    Ok(response)
  }

  /// Serve a file for inline rendering (e.g. a tiptap `<img>` / preview) with
  /// `Content-Disposition: inline` and the real content-type. Authorization: a board-attached file
  /// follows its post's content visibility; an unattached/generic file requires `file:read`.
  pub async fn inline(&self, uuid: Uuid) -> Result<Response<Body>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(file) = Files::find_by_id(uuid).one(&txn).await? else {
      return Err(SumidaError::NoData("File".to_string(), uuid.to_string()));
    };
    self.authorize_inline(&file, &txn).await?;
    txn.commit().await?;

    use axum::http::{StatusCode, header};
    use tokio_util::io::ReaderStream;

    let storage = SumidaStorage::new();
    let path = file.get_path();
    let stream = storage.stream(&path).await?;
    let size = storage.get_file_size(&path).await?;
    let body = Body::from_stream(ReaderStream::new(stream));

    Response::builder()
      .status(StatusCode::OK)
      .header(header::CONTENT_TYPE, file.mime_type)
      .header(header::CONTENT_LENGTH, size.to_string())
      .header(header::CONTENT_DISPOSITION, "inline")
      .body(body)
      .map_err(|_| SumidaError::InternalServerError("Failed to create response".into()))
  }

  /// Visibility gate for [`Self::inline`].
  async fn authorize_inline<C: sea_orm::ConnectionTrait>(
    &self,
    file: &entity::fs::files::Model,
    db: &C,
  ) -> Result<(), SumidaError> {
    use entity::bps::board_files::Column as BoardFileColumn;
    use sea_orm::QueryFilter;

    // A file attached to a post inherits that post's content visibility.
    let attached = BoardFiles::find()
      .filter(BoardFileColumn::FileUuid.eq(file.uuid))
      .filter(BoardFileColumn::PostId.is_not_null())
      .one(db)
      .await?
      .and_then(|bf| bf.post_id);

    if let Some(post_id) = attached {
      let post = BoardPosts::find_by_id(post_id)
        .one(db)
        .await?
        .ok_or_else(|| SumidaError::NoData("BoardPost".into(), post_id.to_string()))?;
      let board = Boards::find_by_id(post.board_id)
        .one(db)
        .await?
        .ok_or_else(|| SumidaError::NoData("Board".into(), post.board_id.to_string()))?;
      let user_id = if self.session.profile.is_authorized { Some(self.session.profile.id) } else { None };

      return if crate::service::bps::board_post::util::check_post_visible(
        user_id,
        post.user_id,
        self.session.profile.is_admin,
        board.content_visibility,
        post.content_visibility,
      )? {
        Ok(())
      } else {
        Err(SumidaError::NoAccessPermission)
      };
    }

    // Unattached/generic file: require `file:read` on the object.
    if self.select(Files::find_by_id(file.uuid)).one(db).await?.is_some() {
      Ok(())
    } else {
      Err(SumidaError::NoAccessPermission)
    }
  }

  pub async fn delete(&self, uuid: Uuid, data: DeleteFile) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(file) = self.select(Files::find_by_id(uuid)).one(&txn).await? else {
      return Err(SumidaError::NoData("File".to_string(), uuid.to_string()));
    };

    if data.hard {
      SumidaStorage::new().delete(&file.get_path()).await?;
    }

    let result = Files::delete_by_id(uuid).exec(&txn).await?;

    // Drop any access policies bound to this file object so they don't outlive the file.
    ap::edit().namespace("file").object(uuid).delete(&txn).await?;

    txn.commit().await?;

    Ok(result)
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
      .expr_as(Func::count(Expr::col(Column::Uuid)), "total_files")
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
      .expr_as(Func::count(Expr::col(Column::Uuid)), "count")
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
      .expr_as(Func::count(Expr::col(Column::Uuid)), "count")
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
}

impl<S: SumidaState> ApiService<ApCondition, Files> for FileService<S> {
  fn default_query_with(query: Select<Files>) -> Select<Files> {
    query //
      .expr(Expr::col((entity::fs::files::Entity, Asterisk)))
      .left_join(Users)
  }

  fn ap(&self) -> ApCondition {
    use entity::fs::files::Column;
    ap::ns("file").who(&self.session).join(Files, Column::Uuid)
  }

  fn select(&self, query: sea_orm::Select<Files>) -> sea_orm::Select<Files> {
    use entity::fs::files::Column;
    self
      .ap()
      .expr(Expr::col((Files, Column::Uuid)).cast_as("varchar"))
      .action("read")
      .apply(Self::default_query_with(query))
  }
}
