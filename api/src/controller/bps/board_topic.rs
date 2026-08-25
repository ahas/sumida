use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{BOARD_TOPIC_TAG, EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/board-topics";

const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct BoardTopicController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardTopicController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The topics a board sorts its posts into.
///
/// A topic belongs to exactly one board, so `boardId` is how a client reads the set it can offer a
/// writer.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = BOARD_TOPIC_TAG,
  operation_id = "board_topic_list",
  params(BoardTopicFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of topics, and how many there are behind it", body = List<BoardTopicListItem>),
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
  Query(filter): Query<BoardTopicFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardTopicListItem>, SumidaError> {
  Ok(
    BoardTopicService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One topic.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_TOPIC_TAG,
  operation_id = "board_topic_get",
  params(("id" = i32, Path, description = "The topic's identifier")),
  responses(
    (status = 200, description = "The topic", body = GetBoardTopic),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBoardTopic, SumidaError> {
  BoardTopicService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardTopic".to_owned(), id.to_string()))
}

/// Add a topic to a board.
///
/// `order` decides where it sits in the board's list; `id` may be sent to choose the identifier.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = BOARD_TOPIC_TAG,
  operation_id = "board_topic_create",
  request_body = CreateBoardTopic,
  responses(
    (status = 201, description = "The topic's identifier", body = BoardTopicCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBoardTopic>,
) -> Result<BoardTopicCreated, SumidaError> {
  Ok(BoardTopicCreated {
    id: BoardTopicService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Change a topic. Only the fields sent are touched, and `boardId` may be sent to move it to
/// another board.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_TOPIC_TAG,
  operation_id = "board_topic_update",
  params(("id" = i32, Path, description = "The topic's identifier")),
  request_body = UpdateBoardTopic,
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
  Json(body): Json<UpdateBoardTopic>,
) -> Result<BoardTopicUpdated, SumidaError> {
  BoardTopicService::new(state, session).update(id, body).await?;
  Ok(BoardTopicUpdated)
}

/// Remove a topic. The posts filed under it keep standing and lose their topic.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_TOPIC_TAG,
  operation_id = "board_topic_delete",
  params(("id" = i32, Path, description = "The topic's identifier")),
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
) -> Result<BoardTopicDeleted, SumidaError> {
  BoardTopicService::new(state, session).delete(id).await?;
  Ok(BoardTopicDeleted)
}

impl<S> ApiController<S> for BoardTopicController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board_topic")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<BoardTopicFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
