use crate::core::error::AppError;
use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};

use axum::{
  extract::{Path, Query, State},
  http::StatusCode,
  routing, Json, Router,
};
use tower_sessions::Session;

async fn list<S>(
  State(_state): State<S>,
  Query(filter): Query<UserFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserListItem>, AppError> {
  Ok(UserService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(username): Path<String>) -> Result<GetUser, AppError> {
  match UserService::get(&username).await? {
    Some(user) => Ok(user),
    None => Err(AppError::NoUserAccount(username.to_string())),
  }
}

async fn create<S>(State(_state): State<S>, Json(body): Json<CreateUser>) -> Result<UserCreated, AppError> {
  let username = body.username.clone();
  UserService::create(body).await?;

  Ok(UserCreated { username })
}

async fn update<S>(
  State(_state): State<S>,
  Path(username): Path<String>,
  Json(body): Json<UpdateUser>,
) -> Result<UserUpdated, AppError> {
  UserService::update(username, body).await?;
  Ok(UserUpdated)
}

async fn delete<S>(State(_state): State<S>, Path(username): Path<String>) -> Result<UserDeleted, AppError> {
  UserService::delete(username).await?;
  Ok(UserDeleted)
}

async fn sign_in<S>(
  session: UserSession,
  State(_state): State<S>,
  Json(body): Json<SignInUser>,
) -> Result<StatusCode, AppError> {
  UserService::sign_in(&session, body).await?;
  Ok(StatusCode::OK)
}

async fn sign_out<S>(session: Session, State(_state): State<S>) -> Result<StatusCode, AppError> {
  session.clear().await;
  Ok(StatusCode::OK)
}

async fn profile<S>(session: UserSession, State(_state): State<S>) -> Result<UserProfile, AppError> {
  let user_data = &session.user_data;

  Ok(UserProfile {
    username: user_data.username.clone(),
    full_name: user_data.full_name.clone(),
    organization: user_data.organization.clone(),
    role: user_data.role.clone(),
    ability: user_data.ability.clone(),
    is_admin: user_data.is_admin,
    is_authorized: user_data.is_authorized,
    first_seen_at: user_data.first_seen_at,
    last_seen_at: user_data.last_seen_at,
  })
}

async fn get_my_info<S>(session: UserSession, State(_state): State<S>) -> Result<GetUser, AppError> {
  match UserService::get(&session.user_data.username).await? {
    Some(user) => Ok(user),
    None => Err(AppError::NoUserAccount(session.user_data.username.to_string())),
  }
}

async fn update_my_info<S>(
  session: UserSession,
  State(_state): State<S>,
  Json(body): Json<UpdateUser>,
) -> Result<UserUpdated, AppError> {
  UserService::update_my_info(&session, body).await?;
  Ok(UserUpdated)
}

async fn invite<S>(
  session: UserSession,
  State(_state): State<S>,
  Path(username): Path<String>,
) -> Result<StatusCode, AppError> {
  UserService::invite(&session, username).await?;
  Ok(StatusCode::OK)
}

async fn activate<S>(
  session: UserSession,
  State(_state): State<S>,
  Json(body): Json<UserActivation>,
) -> Result<StatusCode, AppError> {
  UserService::activate(&session, body).await?;
  Ok(StatusCode::ACCEPTED)
}

async fn change_password<S>(
  _session: UserSession,
  State(_state): State<S>,
  Json(body): Json<ChangePassword>,
) -> Result<StatusCode, AppError> {
  UserService::change_password(body).await?;
  Ok(StatusCode::OK)
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
}
