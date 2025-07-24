use axum::{
  Router,
  extract::{Path, Query, State},
  routing,
};

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};

use super::ApiController;

pub struct UserLogController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> UserLogController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    State(_state): State<S>,
    Query(filter): Query<UserLogFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserLogListItem>, SumidaError> {
    Ok(UserLogService::list(filter, pagination).await?.into())
  }

  async fn get(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetUserLog, SumidaError> {
    UserLogService::get(id).await?.ok_or_else(|| SumidaError::NoData("UserLog".to_owned(), id.to_string()))
  }

  async fn delete(State(_state): State<S>, Path(id): Path<i32>) -> Result<UserLogDeleted, SumidaError> {
    UserLogService::delete(id).await?;
    Ok(UserLogDeleted)
  }
}

impl<S> ApiController<S> for UserLogController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(user_log, read));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(user_log, delete));

    Router::new() //
      .merge(route_read)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
