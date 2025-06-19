use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

async fn list<S>(
  State(_state): State<S>,
  Query(filter): Query<UserBlockFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserBlockListItem>, SumidaError> {
  Ok(UserBlockService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetUserBlock, SumidaError> {
  match UserBlockService::get(id).await? {
    Some(role) => Ok(role),
    None => Err(SumidaError::NoData("Role".to_owned(), id.to_string())),
  }
}

async fn create<S>(
  State(_state): State<S>,
  Json(body): Json<CreateUserBlock>,
) -> Result<UserBlockCreated, SumidaError> {
  Ok(UserBlockCreated {
    id: UserBlockService::create(body).await?.last_insert_id,
  })
}

async fn update<S>(
  State(_state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<UpdateUserBlock>,
) -> Result<UserBlockUpdated, SumidaError> {
  UserBlockService::update(id, body).await?;
  Ok(UserBlockUpdated)
}

async fn delete<S>(State(_state): State<S>, Path(id): Path<i32>) -> Result<UserBlockDeleted, SumidaError> {
  UserBlockService::delete(id).await?;
  Ok(UserBlockDeleted)
}

pub fn new<S>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  let route_read = Router::new()
    .route("/", routing::get(list::<S>))
    .route("/{id}", routing::get(get::<S>))
    .route_layer(ability_guard!(role, read));
  let route_create =
    Router::new().route("/", routing::post(create::<S>)).route_layer(ability_guard!(role, create));
  let route_update =
    Router::new().route("/{id}", routing::patch(update::<S>)).route_layer(ability_guard!(role, update));
  let route_delete =
    Router::new().route("/{id}", routing::delete(delete::<S>)).route_layer(ability_guard!(role, delete));

  Router::new()
    .merge(route_read)
    .merge(route_create)
    .merge(route_update)
    .merge(route_delete)
    .route_layer(user_guard!(state))
}
