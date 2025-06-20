use axum::{
  Json, Router,
  extract::{Path, State},
  http::StatusCode,
  routing,
};
use axum_extra::extract::Query;

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

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
    State(_state): State<S>,
    Query(filter): Query<UserFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserListItem>, SumidaError> {
    Ok(UserService::list(filter, pagination).await?.into())
  }

  async fn get(State(_state): State<S>, Path(username): Path<String>) -> Result<GetUser, SumidaError> {
    match UserService::get(&username).await? {
      Some(user) => Ok(user),
      None => Err(SumidaError::InvalidAccount),
    }
  }

  async fn create(
    State(_state): State<S>,
    Json(body): Json<CreateUser>,
  ) -> Result<UserCreated, SumidaError> {
    let username = body.username.clone();
    UserService::create(body).await?;

    Ok(UserCreated { username })
  }

  async fn update(
    State(_state): State<S>,
    Path(username): Path<String>,
    Json(body): Json<UpdateUser>,
  ) -> Result<UserUpdated, SumidaError> {
    UserService::update(username, body).await?;
    Ok(UserUpdated)
  }

  async fn delete(
    State(_state): State<S>,
    Path(username): Path<String>,
  ) -> Result<UserDeleted, SumidaError> {
    UserService::delete(username).await?;
    Ok(UserDeleted)
  }

  async fn sign_in(
    mut user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<SignInUser>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::sign_in(&mut user_ctx, body).await?;
    Ok(StatusCode::OK)
  }

  async fn sign_out(user_ctx: Session, State(_state): State<S>) -> Result<StatusCode, SumidaError> {
    user_ctx.delete().await.ok();
    Ok(StatusCode::OK)
  }

  async fn profile(user_ctx: UserContext, State(_state): State<S>) -> Result<UserProfile, SumidaError> {
    Ok(user_ctx.profile)
  }

  async fn get_my_info(user_ctx: UserContext, State(_state): State<S>) -> Result<GetUser, SumidaError> {
    match UserService::get(&user_ctx.profile.username).await? {
      Some(user) => Ok(user),
      None => Err(SumidaError::InvalidAccount),
    }
  }

  async fn update_my_info(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<UpdateUser>,
  ) -> Result<UserUpdated, SumidaError> {
    UserService::update_my_info(&user_ctx, body).await?;
    Ok(UserUpdated)
  }

  async fn invite(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(username): Path<String>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::invite(&user_ctx, username).await?;
    Ok(StatusCode::OK)
  }

  async fn activate(
    mut user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<UserActivation>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::activate(&mut user_ctx, body).await?;
    Ok(StatusCode::ACCEPTED)
  }

  async fn change_password(
    _user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<ChangePassword>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::change_password(body).await?;
    Ok(StatusCode::OK)
  }

  async fn impersonate(
    mut user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<Impersonate>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::impersonate(&mut user_ctx, body.username).await?;
    Ok(StatusCode::ACCEPTED)
  }

  async fn get_vapid(user_ctx: UserContext, State(_state): State<S>) -> Result<GetVapid, SumidaError> {
    UserService::get_vapid(&user_ctx).await
  }
}

impl<S> ApiController<S> for UserController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{username}", routing::get(Self::get))
      .route_layer(ability_guard!(user, read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(ability_guard!(user, create));
    let route_update = Router::new()
      .route("/{username}", routing::patch(Self::update))
      .route("/{username}/invite", routing::post(Self::invite))
      .route_layer(ability_guard!(user, update));
    let route_delete = Router::new()
      .route("/{username}", routing::delete(Self::delete))
      .route_layer(ability_guard!(user, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route("/profile", routing::get(Self::profile))
      .route("/my-info", routing::get(Self::get_my_info))
      .route("/my-info", routing::patch(Self::update_my_info))
      .route_layer(user_guard!(state))
      .route("/sign-out", routing::post(Self::sign_out))
      .route("/sign-in", routing::post(Self::sign_in))
      .route("/change-password", routing::post(Self::change_password))
      .route("/activate", routing::post(Self::activate))
      .route("/impersonate", routing::post(Self::impersonate))
      .route("/vapid", routing::get(Self::get_vapid))
  }
}
