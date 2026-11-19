use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, REACTION_TAG, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/board-post-reactions";

const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct BoardPostReactionController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardPostReactionController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The marks readers have left on a post.
///
/// `postId` is not one of the keys that narrow this list — it **chooses** what is being read, and it
/// is answered before the query runs. Leaving it out is a bad request, and a board whose reaction
/// visibility does not admit this caller is refused outright rather than answered with an empty
/// page, which would say the post exists and carries nothing.
///
/// `value` is whatever word the product uses for a mark; nothing here fixes the vocabulary.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = REACTION_TAG,
  operation_id = "board_post_reaction_list",
  params(BoardPostReactionFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of reactions, and how many there are behind it", body = List<BoardPostReactionListItem>),
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
  Query(filter): Query<BoardPostReactionFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardPostReactionListItem>, SumidaError> {
  Ok(
    BoardPostReactionService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One reaction.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = REACTION_TAG,
  operation_id = "board_post_reaction_get",
  params(("id" = i32, Path, description = "The reaction's identifier")),
  responses(
    (status = 200, description = "The reaction", body = GetBoardPostReaction),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBoardPostReaction, SumidaError> {
  BoardPostReactionService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardPostReaction".to_owned(), id.to_string()))
}

/// Leave a mark on a post.
///
/// `userId` may be left null, which is a mark left by nobody in particular — an anonymous board's
/// reaction. The board's own settings decide whether that is allowed.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = REACTION_TAG,
  operation_id = "board_post_reaction_create",
  request_body = CreateBoardPostReaction,
  responses(
    (status = 201, description = "The reaction's identifier", body = BoardPostReactionCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBoardPostReaction>,
) -> Result<BoardPostReactionCreated, SumidaError> {
  Ok(BoardPostReactionCreated {
    id: BoardPostReactionService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Change a mark — which post it is on, whose it is, or what it says.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = REACTION_TAG,
  operation_id = "board_post_reaction_update",
  params(("id" = i32, Path, description = "The reaction's identifier")),
  request_body = UpdateBoardPostReaction,
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
  Json(body): Json<UpdateBoardPostReaction>,
) -> Result<BoardPostReactionUpdated, SumidaError> {
  BoardPostReactionService::new(state, session).update(id, body).await?;
  Ok(BoardPostReactionUpdated)
}

/// Take a mark back.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = REACTION_TAG,
  operation_id = "board_post_reaction_delete",
  params(("id" = i32, Path, description = "The reaction's identifier")),
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
) -> Result<BoardPostReactionDeleted, SumidaError> {
  BoardPostReactionService::new(state, session).delete(id).await?;
  Ok(BoardPostReactionDeleted)
}

impl<S> ApiController<S> for BoardPostReactionController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "board_post_reaction")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<BoardPostReactionFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
