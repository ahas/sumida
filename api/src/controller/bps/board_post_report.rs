use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_router};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

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

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardPostReportFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardPostReportListItem>, SumidaError> {
    Ok(BoardPostReportService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardPostReport, SumidaError> {
    BoardPostReportService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPostReport".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardPostReport>,
  ) -> Result<BoardPostReportCreated, SumidaError> {
    Ok(BoardPostReportCreated {
      id: BoardPostReportService::new(state, session).create(body).await?,
    })
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardPostReportDeleted, SumidaError> {
    BoardPostReportService::new(state, session).delete(id).await?;
    Ok(BoardPostReportDeleted)
  }
}

impl<S> ApiController<S> for BoardPostReportController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    // Reading/resolving reports is moderator-only; filing one is open to any authenticated user
    // that can read the post (enforced in the service).
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "board_post_report", read));
    let route_filters = filter_descriptor_router::<S, BoardPostReportFilter>(state, "board_post_report");
    let route_create = Router::new().route("/", routing::post(Self::create)).route_layer(service_ap!(
      state,
      "board_post_report",
      create
    ));
    let route_delete = Router::new()
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_post_report", delete));

    Router::new()
      .merge(route_read)
      .merge(route_filters)
      .merge(route_create)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
