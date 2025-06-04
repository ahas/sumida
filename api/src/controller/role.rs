use axum::{
  extract::{Path, Query, State},
  routing, Json, Router,
};
use serde::Serialize;

use crate::core::error::SumidaError;
use crate::provider::RoleProvider;
use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

async fn list<S>(
  State(_state): State<S>,
  Query(filter): Query<RoleFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<RoleListItem>, SumidaError> {
  Ok(RoleService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetRole, SumidaError> {
  match RoleService::get(id).await? {
    Some(role) => Ok(role),
    None => Err(SumidaError::NoData("Role".to_owned(), id.to_string())),
  }
}

async fn create<S, T, R>(
  State(_state): State<S>,
  Json(body): Json<CreateRole>,
) -> Result<RoleCreated, SumidaError>
where
  T: Default + Serialize + 'static,
  R: RoleProvider<T> + Clone + Send + Sync + 'static,
{
  Ok(RoleCreated {
    id: RoleService::create::<T, R>(body).await?.last_insert_id,
  })
}

async fn update<S, T, R>(
  State(_state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<UpdateRole>,
) -> Result<RoleUpdated, SumidaError>
where
  T: Default + Serialize + 'static,
  R: RoleProvider<T> + Clone + Send + Sync + 'static,
{
  RoleService::update::<T, R>(id, body).await?;
  Ok(RoleUpdated)
}

async fn delete<S>(State(_state): State<S>, Path(id): Path<i32>) -> Result<RoleDeleted, SumidaError> {
  RoleService::delete(id).await?;
  Ok(RoleDeleted)
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
  let route_create =
    Router::new().route("/", routing::post(create::<S, T, R>)).route_layer(ability_guard!(role, create));
  let route_update = Router::new()
    .route("/{id}", routing::patch(update::<S, T, R>))
    .route_layer(ability_guard!(role, update));
  let route_delete =
    Router::new().route("/{id}", routing::delete(delete::<S>)).route_layer(ability_guard!(role, delete));

  Router::new()
    .merge(route_read)
    .merge(route_create)
    .merge(route_update)
    .merge(route_delete)
    .route_layer(user_guard!(state))
}
