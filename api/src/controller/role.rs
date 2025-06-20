use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use serde::Serialize;

use crate::provider::RoleProvider;
use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

use super::ApiController;

pub struct RoleController<S, T, R> {
  _state: std::marker::PhantomData<S>,
  _ability: std::marker::PhantomData<T>,
  _provider: std::marker::PhantomData<R>,
}

impl<S, T, R> RoleController<S, T, R>
where
  S: Clone + Send + Sync + 'static,
  T: Default + Serialize + 'static,
  R: RoleProvider<T> + Clone + Send + Sync + 'static,
{
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
      _ability: std::marker::PhantomData,
      _provider: std::marker::PhantomData,
    }
  }

  async fn list(
    State(_state): State<S>,
    Query(filter): Query<RoleFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<RoleListItem>, SumidaError> {
    Ok(RoleService::<T, R>::list(filter, pagination).await?.into())
  }

  async fn get(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetRole, SumidaError> {
    match RoleService::<T, R>::get(id).await? {
      Some(role) => Ok(role),
      None => Err(SumidaError::NoData("Role".to_owned(), id.to_string())),
    }
  }

  async fn create(
    State(_state): State<S>,
    Json(body): Json<CreateRole>,
  ) -> Result<RoleCreated, SumidaError> {
    Ok(RoleCreated {
      id: RoleService::<T, R>::create(body).await?.last_insert_id,
    })
  }

  async fn update(
    State(_state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateRole>,
  ) -> Result<RoleUpdated, SumidaError>
  where
    T: Default + Serialize + 'static,
    R: RoleProvider<T> + Clone + Send + Sync + 'static,
  {
    RoleService::<T, R>::update(id, body).await?;
    Ok(RoleUpdated)
  }

  async fn delete(State(_state): State<S>, Path(id): Path<i32>) -> Result<RoleDeleted, SumidaError> {
    RoleService::<T, R>::delete(id).await?;
    Ok(RoleDeleted)
  }
}

impl<S, T, R> ApiController<S> for RoleController<S, T, R>
where
  S: Clone + Send + Sync + 'static,
  T: Default + Serialize + 'static,
  R: RoleProvider<T> + Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(ability_guard!(role, read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(ability_guard!(role, create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(ability_guard!(role, update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(ability_guard!(role, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
