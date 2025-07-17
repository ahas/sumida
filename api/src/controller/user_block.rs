use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};

use super::ApiController;

pub struct UserBlockController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> UserBlockController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    State(_state): State<S>,
    Query(filter): Query<UserBlockFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserBlockListItem>, SumidaError> {
    Ok(UserBlockService::list(filter, pagination).await?.into())
  }

  async fn get(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetUserBlock, SumidaError> {
    match UserBlockService::get(id).await? {
      Some(user_block) => Ok(user_block),
      None => Err(SumidaError::NoData("UserBlock".to_owned(), id.to_string())),
    }
  }

  async fn create(
    State(_state): State<S>,
    Json(body): Json<CreateUserBlock>,
  ) -> Result<UserBlockCreated, SumidaError> {
    Ok(UserBlockCreated {
      id: UserBlockService::create(body).await?.last_insert_id,
    })
  }

  async fn update(
    State(_state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateUserBlock>,
  ) -> Result<UserBlockUpdated, SumidaError> {
    UserBlockService::update(id, body).await?;
    Ok(UserBlockUpdated)
  }

  async fn delete(State(_state): State<S>, Path(id): Path<i32>) -> Result<UserBlockDeleted, SumidaError> {
    UserBlockService::delete(id).await?;
    Ok(UserBlockDeleted)
  }
}

impl<S> ApiController<S> for UserBlockController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(user_block, read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(user_block, create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(user_block, update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(user_block, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
