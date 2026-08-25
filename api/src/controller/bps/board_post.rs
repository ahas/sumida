use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, POST_TAG, Refused, Unreadable, WrongBody};
use crate::service::bps::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/board-posts";

const ONE: &str = "/{id}";
const VIEW: &str = "/{id}/view";
const VIEWERS: &str = "/{id}/viewers";

/// The four kinds one table carries, said once because every operation here depends on it.
const KINDS: &str = "An article, a notice, a comment and a reply are all rows of this one resource, \
told apart by `kind`. A comment and a reply hang from another post through `parentId`; an article \
and a notice hang from a board.";

#[derive(Clone, Debug, Default)]
pub struct BoardPostController<S: SumidaState>(std::marker::PhantomData<S>);

impl<S: SumidaState> BoardPostController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }
}

/// The posts this caller may see.
///
/// `withSnippet` turns on full-text highlighting and the `snippet*` keys shape what comes back in
/// `snippet`; without it that field is null.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_list",
  description = KINDS,
  params(BoardPostFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of posts, and how many there are behind it", body = List<BoardPostListItem>),
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
  Query(filter): Query<BoardPostFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardPostListItem>, SumidaError> {
  BoardPostService::new(state, session).list(filter, conditions, pagination).await
}

/// One post, with its attachments, its comments and their replies.
///
/// **Reading here does not count as a view.** The view counter and the viewer record are written by
/// `POST /board-posts/{id}/view`, so a client that wants a read to count calls that one instead.
///
/// A post kept behind a password answers with its body only through that other route.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_get",
  params(("id" = i32, Path, description = "The post's identifier")),
  responses(
    (status = 200, description = "The post and what hangs from it", body = GetBoardPost),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBoardPost, SumidaError> {
  BoardPostService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))
}

/// Read a post **and record that it was read**.
///
/// It answers with the same body the plain read does, and in addition raises the view count and
/// writes the caller into the post's viewer record. This is the route a reader's screen calls.
///
/// It is a `POST` because it writes, but it is grouped with the read routes rather than the writing
/// ones: recording a view is not editing the post, and requiring the capability to edit would stop
/// ordinary readers from opening it.
///
/// `password` is what a post kept behind one asks for. A post that is not behind a password ignores
/// it.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = VIEW,
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_view",
  params(("id" = i32, Path, description = "The post's identifier")),
  request_body = ViewBoardPost,
  responses(
    (status = 200, description = "The post and what hangs from it", body = GetBoardPost),
    (status = 403, description = "The post is behind a password and none was sent, or the wrong one was", body = ErrorBody),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn view<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<ViewBoardPost>,
) -> Result<GetBoardPost, SumidaError> {
  BoardPostService::new(state, session)
    .view(id, body)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))
}

/// Who has read a post, and when.
///
/// A view recorded for nobody in particular comes back with its account fields null. Whether this
/// can be read at all is the board's `viewerVisibility` setting.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = VIEWERS,
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_viewers",
  params(("id" = i32, Path, description = "The post's identifier"), Pagination),
  responses(
    (status = 200, description = "One page of viewers, and how many there are behind it", body = List<BoardPostViewerItem>),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn viewers<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardPostViewerItem>, SumidaError> {
  BoardPostService::new(state, session).list_viewers(id, pagination).await
}

/// Write a post.
///
/// Which board may be written in is decided per board inside the handler, not by this route's gate.
///
/// `attachedFileUuids` names files already uploaded through `POST /files`; a file may be attached to
/// one post only.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_create",
  description = KINDS,
  request_body = CreateBoardPost,
  responses(
    (status = 201, description = "The post's identifier", body = BoardPostCreated),
    (status = 400, description = "A named file does not exist, or is already attached elsewhere", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBoardPost>,
) -> Result<BoardPostCreated, SumidaError> {
  let res = BoardPostService::new(state, session).create(body).await?;
  Ok(BoardPostCreated {
    id: res.last_insert_id,
  })
}

/// Change a post. Only the fields sent are touched.
///
/// Attachments move by naming them: `attachedFileUuids` adds, `detachedFileUuids` removes, and a
/// field left out changes nothing about what is attached.
///
/// Being the author, or being allowed to moderate the board, is checked inside the handler rather
/// than by this route's gate.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_update",
  params(("id" = i32, Path, description = "The post's identifier")),
  request_body = UpdateBoardPost,
  responses(
    (status = 200, description = "Changed. The body is an empty object and names nothing", body = EmptyObject),
    (status = 400, description = "A named file does not exist, or is already attached elsewhere", body = ErrorBody),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn update<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<UpdateBoardPost>,
) -> Result<BoardPostUpdated, SumidaError> {
  BoardPostService::new(state, session).update(id, body).await?;
  Ok(BoardPostUpdated)
}

/// Remove a post, and with it the comments and replies hanging from it.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = POST_TAG,
  operation_id = "board_post_delete",
  params(("id" = i32, Path, description = "The post's identifier")),
  responses(
    (status = 200, description = "Removed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<BoardPostDeleted, SumidaError> {
  BoardPostService::new(state, session).delete(id).await?;
  Ok(BoardPostDeleted)
}

impl<S> ApiController<S> for BoardPostController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board_post")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>))
          .route(VIEW, routing::post(view::<S>))
          .route(VIEWERS, routing::get(viewers::<S>)),
      )
      .descriptor::<BoardPostFilter>()
      // Board-level permissions are checked in the service.
      .create(Router::new().route("/", routing::post(create::<S>)))
      // Ownership/moderate permissions are checked in the service.
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
