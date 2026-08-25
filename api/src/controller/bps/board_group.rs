use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{BOARD_GROUP_TAG, EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/board-groups";

const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct BoardGroupController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardGroupController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The tree boards are listed under.
///
/// A group holds boards and may hold other groups. `parentId` chooses which level is read, and
/// saying nothing reads the roots rather than every group at once — `search` is what looks through
/// the whole tree.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = BOARD_GROUP_TAG,
  operation_id = "board_group_list",
  params(BoardGroupFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of groups, and how many there are behind it", body = List<BoardGroupListItem>),
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
  Query(filter): Query<BoardGroupFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardGroupListItem>, SumidaError> {
  Ok(
    BoardGroupService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One group.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_GROUP_TAG,
  operation_id = "board_group_get",
  params(("id" = i32, Path, description = "The group's identifier")),
  responses(
    (status = 200, description = "The group", body = GetBoardGroup),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBoardGroup, SumidaError> {
  BoardGroupService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardGroup".to_owned(), id.to_string()))
}

/// Add a group.
///
/// `parentId` puts it under another group; left out, it stands at the root. `id` may be sent to
/// choose the identifier, which is what a seeding script does.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = BOARD_GROUP_TAG,
  operation_id = "board_group_create",
  request_body = CreateBoardGroup,
  responses(
    (status = 201, description = "The group's identifier", body = BoardGroupCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBoardGroup>,
) -> Result<BoardGroupCreated, SumidaError> {
  Ok(BoardGroupCreated {
    id: BoardGroupService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Change a group. Only the fields sent are touched; sending `parentId` moves it in the tree.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_GROUP_TAG,
  operation_id = "board_group_update",
  params(("id" = i32, Path, description = "The group's identifier")),
  request_body = UpdateBoardGroup,
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
  Json(body): Json<UpdateBoardGroup>,
) -> Result<BoardGroupUpdated, SumidaError> {
  BoardGroupService::new(state, session).update(id, body).await?;
  Ok(BoardGroupUpdated)
}

/// Remove a group. The boards and groups under it lose the level they hung from.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = BOARD_GROUP_TAG,
  operation_id = "board_group_delete",
  params(("id" = i32, Path, description = "The group's identifier")),
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
) -> Result<BoardGroupDeleted, SumidaError> {
  BoardGroupService::new(state, session).delete(id).await?;
  Ok(BoardGroupDeleted)
}

impl<S> ApiController<S> for BoardGroupController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board_group")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<BoardGroupFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
