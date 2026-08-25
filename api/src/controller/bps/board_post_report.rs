use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, REPORT_TAG, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/board-post-reports";

const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct BoardPostReportController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardPostReportController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The posts readers have flagged.
///
/// This is the moderator's side: reading a report names the post and the reader who filed it, so it
/// stands behind the read capability. Filing one does not.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = REPORT_TAG,
  operation_id = "board_post_report_list",
  params(BoardPostReportFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of reports, and how many there are behind it", body = List<BoardPostReportListItem>),
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
  Query(filter): Query<BoardPostReportFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BoardPostReportListItem>, SumidaError> {
  Ok(
    BoardPostReportService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One report.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = REPORT_TAG,
  operation_id = "board_post_report_get",
  params(("id" = i32, Path, description = "The report's identifier")),
  responses(
    (status = 200, description = "The report", body = GetBoardPostReport),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBoardPostReport, SumidaError> {
  BoardPostReportService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("BoardPostReport".to_owned(), id.to_string()))
}

/// Flag a post.
///
/// The reader is the caller — there is no field for whose report it is. Any account that can read
/// the post may file one, which is checked inside the handler rather than by the route's gate.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = REPORT_TAG,
  operation_id = "board_post_report_create",
  request_body = CreateBoardPostReport,
  responses(
    (status = 201, description = "The report's identifier", body = BoardPostReportCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBoardPostReport>,
) -> Result<BoardPostReportCreated, SumidaError> {
  Ok(BoardPostReportCreated {
    id: BoardPostReportService::new(state, session).create(body).await?,
  })
}

/// Close a report by removing it. There is no route that marks one resolved and keeps it — this is
/// how a moderator is done with one.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = REPORT_TAG,
  operation_id = "board_post_report_delete",
  params(("id" = i32, Path, description = "The report's identifier")),
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
) -> Result<BoardPostReportDeleted, SumidaError> {
  BoardPostReportService::new(state, session).delete(id).await?;
  Ok(BoardPostReportDeleted)
}

impl<S> ApiController<S> for BoardPostReportController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    // Reading/resolving reports is moderator-only; filing one is open to any authenticated user
    // that can read the post (enforced in the service).
    ControllerRouter::new(state, "board_post_report")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<BoardPostReportFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
