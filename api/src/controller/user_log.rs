use axum::extract::{Path, Query, State};
use axum::{Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct UserLogController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> UserLogController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<UserLogFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserLogListItem>, SumidaError> {
    Ok(UserLogService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetUserLog, SumidaError> {
    UserLogService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserLog".to_owned(), id.to_string()))
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<UserLogDeleted, SumidaError> {
    UserLogService::new(state, session).delete(id).await?;
    Ok(UserLogDeleted)
  }
}

impl<S> ApiController<S> for UserLogController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "user_log", read));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "user_log", delete));

    Router::new() //
      .merge(route_read)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
