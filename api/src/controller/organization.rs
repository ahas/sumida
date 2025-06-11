use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

async fn list<S>(
  State(_state): State<S>,
  Query(filter): Query<OrganizationFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<OrganizationListItem>, SumidaError> {
  Ok(OrganizationService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetOrganization, SumidaError> {
  match OrganizationService::get(id).await? {
    Some(org) => Ok(org),
    None => Err(SumidaError::NoData("Organization".to_owned(), id.to_string())),
  }
}

async fn create<S>(
  State(_state): State<S>,
  Json(body): Json<CreateOrganization>,
) -> Result<OrganizationCreated, SumidaError> {
  Ok(OrganizationCreated {
    id: OrganizationService::create(body).await?.last_insert_id,
  })
}

async fn update<S>(
  State(_state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<UpdateOrganization>,
) -> Result<OrganizationUpdated, SumidaError> {
  OrganizationService::update(id, body).await?;
  Ok(OrganizationUpdated)
}

async fn delete<S>(
  State(_state): State<S>,
  Path(id): Path<i32>,
) -> Result<OrganizationDeleted, SumidaError> {
  OrganizationService::delete(id).await?;
  Ok(OrganizationDeleted)
}

pub fn new<S>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  let route_read = Router::new()
    .route("/", routing::get(list::<S>))
    .route("/{id}", routing::get(get::<S>))
    .route_layer(ability_guard!(organization, read));
  let route_create =
    Router::new().route("/", routing::post(create::<S>)).route_layer(ability_guard!(organization, create));
  let route_update = Router::new()
    .route("/{id}", routing::patch(update::<S>))
    .route_layer(ability_guard!(organization, update));
  let route_delete = Router::new()
    .route("/{id}", routing::delete(delete::<S>))
    .route_layer(ability_guard!(organization, delete));

  Router::new()
    .merge(route_read)
    .merge(route_create)
    .merge(route_update)
    .merge(route_delete)
    .route_layer(user_guard!(state))
}
