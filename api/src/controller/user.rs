use axum::{
  Json, Router,
  extract::{Path, State},
  http::StatusCode,
  routing,
};
use axum_extra::extract::Query;

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};

use super::ApiController;

pub struct UserController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> UserController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    user_ctx: UserContext,
    State(_state): State<S>,
    Query(filter): Query<UserFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserListItem>, SumidaError> {
    Ok(UserService::new(user_ctx).list(filter, pagination).await?.into())
  }

  async fn get(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<GetUser, SumidaError> {
    UserService::new(user_ctx).get(user_id).await?.ok_or(SumidaError::InvalidAccount)
  }

  async fn create(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<CreateUser>,
  ) -> Result<UserCreated, SumidaError> {
    Ok(UserCreated {
      id: UserService::new(user_ctx).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateUser>,
  ) -> Result<UserUpdated, SumidaError> {
    UserService::new(user_ctx).update(id, body).await?;
    Ok(UserUpdated)
  }

  async fn delete(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<UserDeleted, SumidaError> {
    UserService::new(user_ctx).delete(user_id).await?;
    Ok(UserDeleted)
  }

  async fn invite(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(user_ctx).invite(user_id).await?;
    Ok(StatusCode::OK)
  }

  async fn activate(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<UserActivation>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(user_ctx).activate(body).await?;
    Ok(StatusCode::ACCEPTED)
  }

  async fn change_password(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<ChangePassword>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(user_ctx).change_password(body).await?;
    Ok(StatusCode::OK)
  }

  async fn impersonate(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(user_ctx).impersonate(user_id).await?;
    Ok(StatusCode::ACCEPTED)
  }

  async fn get_vapid(user_ctx: UserContext, State(_state): State<S>) -> Result<GetVapid, SumidaError> {
    UserService::new(user_ctx).get_vapid().await
  }
}

impl<S> ApiController<S> for UserController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{user_id}", routing::get(Self::get))
      .route_layer(service_ap!(user, read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(user, create));
    let route_update = Router::new()
      .route("/{user_id}", routing::patch(Self::update))
      .route("/{user_id}/invite", routing::post(Self::invite))
      .route_layer(service_ap!(user, update));
    let route_delete = Router::new()
      .route("/{user_id}", routing::delete(Self::delete))
      .route_layer(service_ap!(user, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
      .route("/change-password", routing::post(Self::change_password))
      .route("/activate", routing::post(Self::activate))
      .route("/{user_id}/impersonate", routing::post(Self::impersonate))
      .route("/vapid", routing::get(Self::get_vapid))
  }
}
