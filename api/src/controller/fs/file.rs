use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::response::Response;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use uuid::Uuid;

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, FILE_TAG, NotFound, Refused, Unreadable, WrongBody};
use crate::service::fs::dto::*;
use crate::service::fs::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/files";

const ONE: &str = "/{uuid}";
const STATS: &str = "/stats";
const DOWNLOAD: &str = "/{uuid}/download";
const INLINE: &str = "/{uuid}/inline";

#[derive(Clone, Debug, Default)]
pub struct FileController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> FileController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The parts an upload is made of. The handler reads them itself rather than through an extractor,
/// so the shape is written out here beside it.
#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
pub struct FileUploadForm {
  /// The bytes. Required; the file name and content type are taken from this part.
  #[schema(value_type = String, format = Binary)]
  pub file:     String,
  /// A JSON object, sent as a text part. Required — an upload without it is refused even though
  /// every key inside it may be left out.
  pub metadata: FileUploadOptions,
}

/// The uploads this installation holds, as records.
///
/// A row is the file's metadata, never its bytes; `GET /files/{uuid}/download` is what serves those.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_list",
  params(FileFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of files, and how many there are behind it", body = List<FileListItem>),
    (
      status = 400,
      description = "A query-string value is not of the shape the reader takes (plain text), or \
                     the condition tree names a key, an operator or a size this list does not \
                     accept (error body).",
      content((String = "text/plain"), (ErrorBody = "application/json")),
    ),
    Refused,
  ),
))]
async fn list<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<FileFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<FileListItem>, SumidaError> {
  Ok(FileService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One file's record, with the grants written against it.
///
/// `refs` counts what points at this file. A file nothing points at is what a storage sweep is
/// allowed to remove.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_get",
  params(("uuid" = Uuid, Path, description = "The file's identifier")),
  responses(
    (status = 200, description = "The file's record", body = GetFile),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(uuid): Path<Uuid>,
) -> Result<GetFile, SumidaError> {
  FileService::new(state, session)
    .get(uuid)
    .await?
    .ok_or_else(|| SumidaError::NoData("File".to_owned(), uuid.to_string()))
}

/// Upload a file. `multipart/form-data`, with a part named `file` and a part named `metadata`.
///
/// `metadata` is a JSON object sent as a text part; both parts are required, and a request missing
/// either is refused. What comes back is the identifier the record is filed under — the bytes are
/// read back through `download` or `inline`.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_upload",
  request_body(content = FileUploadForm, content_type = "multipart/form-data"),
  responses(
    (status = 201, description = "The identifier the file was stored under", body = FileUploaded),
    (
      status = 400,
      description = "The body is not `multipart/form-data`, a part is missing, or `metadata` is \
                     not readable as JSON.",
      body = ErrorBody,
    ),
    (status = 413, description = "The file is over the size this deployment accepts", body = ErrorBody),
    (status = 415, description = "The file's type is not one this deployment accepts", body = ErrorBody),
    Refused,
  ),
))]
async fn upload<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  request: Request,
) -> Result<FileUploaded, SumidaError> {
  let file_upload_data = UploadFile::from_request(state.clone(), request).await?;
  let result = FileService::new(state, session).upload(file_upload_data).await?;

  Ok(result)
}

/// Remove a file.
///
/// `hard` decides which removal it is. Left out or false, the record is marked deleted and the
/// bytes stay where they are; true, the bytes go too and nothing brings them back.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_delete",
  params(("uuid" = Uuid, Path, description = "The file's identifier")),
  request_body = DeleteFile,
  responses(
    (status = 200, description = "Removed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(uuid): Path<Uuid>,
  Json(body): Json<DeleteFile>,
) -> Result<FileDeleted, SumidaError> {
  FileService::new(state, session).delete(uuid, body).await?;

  Ok(FileDeleted)
}

/// What the uploads add up to under the same narrowing the list takes — a count and a total size,
/// broken down by media type and by status.
///
/// Only the flat filter applies here; there is no condition tree and no paging.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = STATS,
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_stats",
  params(FileFilter),
  responses(
    (status = 200, description = "Counts and sizes for the narrowed set", body = FileStats),
    Unreadable,
    Refused,
  ),
))]
async fn stats<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<FileFilter>,
) -> Result<Json<FileStats>, SumidaError> {
  let stats = FileService::new(state, session).get_stats(filter).await?;
  Ok(Json(stats))
}

/// The bytes, as a download.
///
/// The answer carries the file's own content type and `Content-Disposition: attachment` with the
/// name it was uploaded under, so a browser saves it rather than showing it.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = DOWNLOAD,
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_download",
  params(("uuid" = Uuid, Path, description = "The file's identifier")),
  responses(
    (
      status = 200,
      description = "The bytes. `Content-Type` is the file's own and `Content-Disposition` is \
                     `attachment` with the original file name.",
      content_type = "application/octet-stream",
      body = String,
    ),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn download<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(uuid): Path<Uuid>,
) -> Result<Response<Body>, SumidaError> {
  FileService::new(state.clone(), session.clone()).download(uuid).await
}

/// The bytes, for showing in place — an `<img>` in a post body, a preview pane.
///
/// **This route is the one exception to the gate the others stand behind.** It takes the session
/// guard and no capability check, because who may see the bytes is decided per file inside the
/// handler: a file attached to a post follows that post's visibility, and a file attached to
/// nothing needs the ordinary read capability. Gating it on `file:read` would hide the images in a
/// post from every reader allowed to read the post.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = INLINE,
  context_path = MOUNT,
  tag = FILE_TAG,
  operation_id = "file_inline",
  params(("uuid" = Uuid, Path, description = "The file's identifier")),
  responses(
    (
      status = 200,
      description = "The bytes. `Content-Type` is the file's own and `Content-Disposition` is \
                     `inline`.",
      content_type = "application/octet-stream",
      body = String,
    ),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn inline<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(uuid): Path<Uuid>,
) -> Result<Response<Body>, SumidaError> {
  FileService::new(state, session).inline(uuid).await
}

impl<S> ApiController<S> for FileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "file")
      .read(
        Router::new()
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>))
          .route(STATS, routing::get(stats::<S>))
          .route(DOWNLOAD, routing::get(download::<S>)),
      )
      .descriptor::<FileFilter>()
      .create(Router::new().route("/", routing::post(upload::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      // Inline serving authorizes per-row (post visibility for board files, `file:read` otherwise),
      // so it takes the session guard only — not the `file:read` service gate.
      .ungated(Router::new().route(INLINE, routing::get(inline::<S>)))
      .build()
  }
}
