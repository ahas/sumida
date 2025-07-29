use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};

use super::ApiController;

pub struct RoleController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> RoleController<S>
where
  S: Clone + Send + Sync + 'static,
{
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    user_ctx: UserContext,
    State(_state): State<S>,
    Query(filter): Query<RoleFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<RoleListItem>, SumidaError> {
    Ok(RoleService::new(user_ctx).list(filter, pagination).await?.into())
  }

  async fn get(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetRole, SumidaError> {
    RoleService::new(user_ctx)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Role".to_owned(), id.to_string()))
  }

  async fn create(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<CreateRole>,
  ) -> Result<RoleCreated, SumidaError> {
    Ok(RoleCreated {
      id: RoleService::new(user_ctx).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateRole>,
  ) -> Result<RoleUpdated, SumidaError> {
    RoleService::new(user_ctx).update(id, body).await?;
    Ok(RoleUpdated)
  }

  async fn delete(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<RoleDeleted, SumidaError> {
    RoleService::new(user_ctx).delete(id).await?;
    Ok(RoleDeleted)
  }
}

impl<S> ApiController<S> for RoleController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(role, read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(role, create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(role, update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(role, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
