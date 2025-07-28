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
    user_ctx: UserContext,
    State(_state): State<S>,
    Query(filter): Query<UserBlockFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserBlockListItem>, SumidaError> {
    Ok(UserBlockService::list(&user_ctx, filter, pagination).await?.into())
  }

  async fn get(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetUserBlock, SumidaError> {
    UserBlockService::get(&user_ctx, id)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserBlock".to_owned(), id.to_string()))
  }

  async fn create(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<CreateUserBlock>,
  ) -> Result<UserBlockCreated, SumidaError> {
    Ok(UserBlockCreated {
      id: UserBlockService::create(&user_ctx, body).await?.last_insert_id,
    })
  }

  async fn update(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateUserBlock>,
  ) -> Result<UserBlockUpdated, SumidaError> {
    UserBlockService::update(&user_ctx, id, body).await?;
    Ok(UserBlockUpdated)
  }

  async fn delete(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<UserBlockDeleted, SumidaError> {
    UserBlockService::delete(&user_ctx, id).await?;
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
