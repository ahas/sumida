use axum::{
  Router,
  extract::{Path, Query, State},
  routing,
};
use serde::Serialize;

use crate::provider::RoleProvider;
use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

async fn list<S>(
  State(_state): State<S>,
  Query(filter): Query<UserSignInLogFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserSignInLogListItem>, SumidaError> {
  Ok(UserSignInLogService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetUserSignInLog, SumidaError> {
  match UserSignInLogService::get(id).await? {
    Some(role) => Ok(role),
    None => Err(SumidaError::NoData("Role".to_owned(), id.to_string())),
  }
}

async fn delete<S>(
  State(_state): State<S>,
  Path(id): Path<i32>,
) -> Result<UserSignInLogDeleted, SumidaError> {
  UserSignInLogService::delete(id).await?;
  Ok(UserSignInLogDeleted)
}

pub fn new<S, T, R>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
  T: Default + Serialize + 'static,
  R: RoleProvider<T> + Clone + Send + Sync + 'static,
{
  let route_read = Router::new()
    .route("/", routing::get(list::<S>))
    .route("/{id}", routing::get(get::<S>))
    .route_layer(ability_guard!(role, read));
  let route_delete =
    Router::new().route("/{id}", routing::delete(delete::<S>)).route_layer(ability_guard!(role, delete));

  Router::new() //
    .merge(route_read)
    .merge(route_delete)
    .route_layer(user_guard!(state))
}
