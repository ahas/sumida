use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
use entity::bps::board_posts::BoardPostKind;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm_crud::OneOrMany;

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{BOARD_TAG, EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::bps::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/boards";

const ONE: &str = "/{id}";
const POSTS: &str = "/{id}/posts";

#[derive(Clone, Debug, Default)]
pub struct BoardController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> BoardController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// The boards this caller may see.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = BOARD_TAG,
  operation_id = "board_list",
  params(BoardFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of boards, and how many there are behind it", body = List<BoardListItem>),
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
  Query(filter): Query<BoardFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardListItem>, SumidaError> {
  Ok(BoardService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One board, with the settings a client needs before it lets anyone write in it — what may be
/// attached, how large, and who gets to see reactions, viewers and content.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_TAG,
  operation_id = "board_get",
  params(("id" = i32, Path, description = "The board's identifier")),
  responses(
    (status = 200, description = "The board and its settings", body = GetBoard),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBoard, SumidaError> {
  BoardService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("Board".into(), id.to_string()))
}

/// The posts in this board.
///
/// `kind` chooses which of the four the page is made of, and left out it is `article`. It is not a
/// narrowing of one list — each kind is read by its own query, so a comment and an article do not
/// come back on the same page.
///
/// A `boardId` in the query string is **overwritten** by the board in the path. This route always
/// answers for the board it names.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = POSTS,
  context_path = MOUNT,
  tag = BOARD_TAG,
  operation_id = "board_posts",
  params(
    ("id" = i32, Path, description = "The board's identifier"),
    BoardPostFilter,
    ConditionQuery,
    Pagination,
  ),
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
async fn posts<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(board_id): Path<i32>,
  Query(mut filter): Query<BoardPostFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardPostListItem>, SumidaError> {
  // The route is `/boards/{id}/posts`; scope to that board regardless of any query `boardId`.
  filter.board_id = Some(OneOrMany::One(board_id));

  let service = BoardPostService::new(state, session);

  let post_list = match filter.kind.as_ref().unwrap_or(&BoardPostKind::Article) {
    BoardPostKind::Comment => service.get_comments(filter, conditions, pagination).await?,
    BoardPostKind::Reply => service.get_replies(filter, conditions, pagination).await?,
    BoardPostKind::Notice => service.get_notices(filter, conditions, pagination).await?,
    _ => service.get_articles(filter, conditions, pagination).await?,
  };

  Ok(post_list)
}

/// Open a board.
///
/// `topics` creates the board's topics along with it. `id` may be sent to choose the identifier,
/// which is what a seeding script does; left out, one is assigned.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = BOARD_TAG,
  operation_id = "board_create",
  request_body = CreateBoard,
  responses(
    (status = 201, description = "The board's identifier", body = BoardCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBoard>,
) -> Result<BoardCreated, SumidaError> {
  Ok(BoardCreated {
    id: BoardService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Change a board. Only the fields sent are touched, and `topics` replaces the whole set when sent.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_TAG,
  operation_id = "board_update",
  params(("id" = i32, Path, description = "The board's identifier")),
  request_body = UpdateBoard,
  responses(
    (status = 200, description = "Changed. The body is an empty object and names nothing", body = EmptyObject),
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
  Json(body): Json<UpdateBoard>,
) -> Result<BoardUpdated, SumidaError> {
  BoardService::new(state, session).update(id, body).await?;
  Ok(BoardUpdated)
}

/// Remove a board, and with it the posts written in it.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_TAG,
  operation_id = "board_delete",
  params(("id" = i32, Path, description = "The board's identifier")),
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
) -> Result<BoardDeleted, SumidaError> {
  BoardService::new(state, session).delete(id).await?;
  Ok(BoardDeleted)
}

impl<S> ApiController<S> for BoardController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>))
          .route(POSTS, routing::get(posts::<S>)),
      )
      .descriptor::<BoardFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
