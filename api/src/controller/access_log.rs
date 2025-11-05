use axum::extract::{Path, Query, State};
use axum::{Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct AccessLogController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> AccessLogController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<AccessLogFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<AccessLogListItem>, SumidaError> {
    Ok(AccessLogService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetAccessLog, SumidaError> {
    AccessLogService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("AccessLog".to_owned(), id.to_string()))
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<AccessLogDeleted, SumidaError> {
    AccessLogService::new(state, session).delete(id).await?;
    Ok(AccessLogDeleted)
  }
}

impl<S> ApiController<S> for AccessLogController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "access_log", read));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "access_log", delete));

    Router::new() //
      .merge(route_read)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
