use axum::extract::{Path, Query, Request, State};
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use uuid::Uuid;

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{BOARD_FILE_TAG, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::bps::board_file::BoardFileService;
use crate::service::bps::board_file::dto::*;
use crate::service::fs::dto::{FileUploadTrait, FileUploaded};
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/board-files";

const FILE: &str = "/file";

#[derive(Clone, Debug, Default)]
pub struct BoardFileController<S: SumidaState>(std::marker::PhantomData<S>);

impl<S: SumidaState> BoardFileController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }
}

/// The parts an attachment upload is made of. The handler reads them itself rather than through an
/// extractor, so the shape is written out here beside it.
#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
pub struct BoardFileUploadForm {
  /// Which board the attachment is being uploaded for, sent as a text part holding a number.
  /// Required.
  #[schema(value_type = i32)]
  pub board_id: String,
  /// The bytes. Required; the file name and content type are taken from this part.
  #[schema(value_type = String, format = Binary)]
  pub file:     String,
  /// A JSON object, sent as a text part. Required — an upload without it is refused even though
  /// every key inside it may be left out.
  pub metadata: crate::service::fs::dto::FileUploadOptions,
}

/// The files attached to posts.
///
/// A row is a file **and** the post it hangs from: everything the file resource reports is here as
/// well, flattened, with `postId` beside it. The `id` key narrows by the file's uuid.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = BOARD_FILE_TAG,
  operation_id = "board_file_list",
  params(BoardFileFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of attachments, and how many there are behind it", body = List<BoardFileListItem>),
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
  Query(filter): Query<BoardFileFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardFileListItem>, SumidaError> {
  Ok(BoardFileService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One attachment, named by its post and its file.
///
/// **This route cannot be satisfied as it is mounted.** The handler asks the path for a post
/// identifier and a file uuid, and the path it is mounted at declares neither, so every request —
/// however well formed — is answered `500` before the handler body runs. Read one attachment
/// through the list above, narrowed by `id`, until the mounted path carries the two segments the
/// handler reads.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = FILE,
  context_path = MOUNT,
  tag = BOARD_FILE_TAG,
  operation_id = "board_file_get",
  responses(
    (
      status = 500,
      description = "The path declares none of the two segments the handler reads, so nothing this \
                     route is sent can succeed.",
      body = ErrorBody,
    ),
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path((post_id, file_uuid)): Path<(i32, Uuid)>,
) -> Result<GetBoardFile, SumidaError> {
  BoardFileService::new(state, session)
    .get(post_id, file_uuid)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardFile".into(), format!("{post_id}-{file_uuid}")))
}

/// Upload a file as a post attachment. `multipart/form-data`, with `board_id`, `file` and
/// `metadata` parts.
///
/// It answers with the **file's** identifier, the same shape `POST /files` answers with — not with
/// an attachment record. Nothing is attached to a post yet at this point: the uuid is what a post
/// then names in its `attachedFileUuids`.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = BOARD_FILE_TAG,
  operation_id = "board_file_upload",
  request_body(content = BoardFileUploadForm, content_type = "multipart/form-data"),
  responses(
    (status = 201, description = "The identifier the file was stored under", body = FileUploaded),
    (
      status = 400,
      description = "The body is not `multipart/form-data`, a part is missing, `board_id` is not a \
                     number, or `metadata` is not readable as JSON.",
      body = ErrorBody,
    ),
    (status = 413, description = "The file is over the size this board accepts", body = ErrorBody),
    (status = 415, description = "The file's type is not one this board accepts", body = ErrorBody),
    Refused,
  ),
))]
async fn upload<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  request: Request,
) -> Result<FileUploaded, SumidaError> {
  let file_upload_data = UploadBoardFile::from_request(state.clone(), request).await?;
  let result = BoardFileService::new(state, session).upload(file_upload_data).await?;

  Ok(result)
}

/// Detach a file from a post.
///
/// Which attachment is named in the **body**, not in the path: `postId` and `fileUuid` together.
/// `hard` decides whether the file's bytes go with it or only the attachment does.
///
/// The answer's body is the JSON literal `null`, not an object — the one route in this fragment
/// that answers that way.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = "",
  context_path = MOUNT,
  tag = BOARD_FILE_TAG,
  operation_id = "board_file_delete",
  request_body = DeleteBoardFile,
  responses(
    (status = 200, description = "Detached. The body is the JSON literal `null`", content_type = "application/json"),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<DeleteBoardFile>,
) -> Result<BoardFileDeleted, SumidaError> {
  BoardFileService::new(state, session).delete(body).await?;

  Ok(BoardFileDeleted)
}

impl<S> ApiController<S> for BoardFileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board_file")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(FILE, routing::get(get::<S>)),
      )
      .descriptor::<BoardFileFilter>()
      .create(Router::new().route("/", routing::post(upload::<S>)))
      .delete(Router::new().route("/", routing::delete(delete::<S>)))
      .build()
  }
}
