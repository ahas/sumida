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

async fn list<S>(
  State(_state): State<S>,
  Query(filter): Query<UserFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserListItem>, SumidaError> {
  Ok(UserService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(username): Path<String>) -> Result<GetUser, SumidaError> {
  match UserService::get(&username).await? {
    Some(user) => Ok(user),
    None => Err(SumidaError::InvalidAccount),
  }
}

async fn create<S>(
  State(_state): State<S>,
  Json(body): Json<CreateUser>,
) -> Result<UserCreated, SumidaError> {
  let username = body.username.clone();
  UserService::create(body).await?;

  Ok(UserCreated { username })
}

async fn update<S>(
  State(_state): State<S>,
  Path(username): Path<String>,
  Json(body): Json<UpdateUser>,
) -> Result<UserUpdated, SumidaError> {
  UserService::update(username, body).await?;
  Ok(UserUpdated)
}

async fn delete<S>(
  State(_state): State<S>,
  Path(username): Path<String>,
) -> Result<UserDeleted, SumidaError> {
  UserService::delete(username).await?;
  Ok(UserDeleted)
}

async fn sign_in<S>(
  mut user_ctx: UserContext,
  State(_state): State<S>,
  Json(body): Json<SignInUser>,
) -> Result<StatusCode, SumidaError> {
  UserService::sign_in(&mut user_ctx, body).await?;
  Ok(StatusCode::OK)
}

async fn sign_out<S>(user_ctx: Session, State(_state): State<S>) -> Result<StatusCode, SumidaError> {
  user_ctx.delete().await.ok();
  Ok(StatusCode::OK)
}

async fn profile<S>(user_ctx: UserContext, State(_state): State<S>) -> Result<UserProfile, SumidaError> {
  Ok(user_ctx.profile)
}

async fn get_my_info<S>(user_ctx: UserContext, State(_state): State<S>) -> Result<GetUser, SumidaError> {
  match UserService::get(&user_ctx.profile.username).await? {
    Some(user) => Ok(user),
    None => Err(SumidaError::InvalidAccount),
  }
}

async fn update_my_info<S>(
  user_ctx: UserContext,
  State(_state): State<S>,
  Json(body): Json<UpdateUser>,
) -> Result<UserUpdated, SumidaError> {
  UserService::update_my_info(&user_ctx, body).await?;
  Ok(UserUpdated)
}

async fn invite<S>(
  user_ctx: UserContext,
  State(_state): State<S>,
  Path(username): Path<String>,
) -> Result<StatusCode, SumidaError> {
  UserService::invite(&user_ctx, username).await?;
  Ok(StatusCode::OK)
}

async fn activate<S>(
  mut user_ctx: UserContext,
  State(_state): State<S>,
  Json(body): Json<UserActivation>,
) -> Result<StatusCode, SumidaError> {
  UserService::activate(&mut user_ctx, body).await?;
  Ok(StatusCode::ACCEPTED)
}

async fn change_password<S>(
  _user_ctx: UserContext,
  State(_state): State<S>,
  Json(body): Json<ChangePassword>,
) -> Result<StatusCode, SumidaError> {
  UserService::change_password(body).await?;
  Ok(StatusCode::OK)
}

async fn impersonate<S>(
  mut user_ctx: UserContext,
  State(_state): State<S>,
  Json(body): Json<Impersonate>,
) -> Result<StatusCode, SumidaError> {
  UserService::impersonate(&mut user_ctx, body.username).await?;
  Ok(StatusCode::ACCEPTED)
}

async fn get_vapid<S>(user_ctx: UserContext, State(_state): State<S>) -> Result<GetVapid, SumidaError> {
  UserService::get_vapid(&user_ctx).await
}

pub fn new<S>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  let route_read = Router::new()
    .route("/", routing::get(list::<S>))
    .route("/{username}", routing::get(get::<S>))
    .route_layer(ability_guard!(user, read));
  let route_create =
    Router::new().route("/", routing::post(create::<S>)).route_layer(ability_guard!(user, create));
  let route_update = Router::new()
    .route("/{username}", routing::patch(update::<S>))
    .route("/{username}/invite", routing::post(invite::<S>))
    .route_layer(ability_guard!(user, update));
  let route_delete = Router::new()
    .route("/{username}", routing::delete(delete::<S>))
    .route_layer(ability_guard!(user, delete));

  Router::new()
    .merge(route_read)
    .merge(route_create)
    .merge(route_update)
    .merge(route_delete)
    .route("/profile", routing::get(profile::<S>))
    .route("/my-info", routing::get(get_my_info::<S>))
    .route("/my-info", routing::patch(update_my_info::<S>))
    .route_layer(user_guard!(state))
    .route("/sign-out", routing::post(sign_out::<S>))
    .route("/sign-in", routing::post(sign_in::<S>))
    .route("/change-password", routing::post(change_password::<S>))
    .route("/activate", routing::post(activate::<S>))
    .route("/impersonate", routing::post(impersonate::<S>))
    .route("/vapid", routing::get(get_vapid::<S>))
}
