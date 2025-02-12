use axum::{
  extract::{Path, Query, State},
  routing, Json, Router,
};
use uuid::Uuid;

use crate::core::error::AppError;
use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{admin_guard, user_guard};

async fn list<S>(
  _session: UserSession,
  State(_state): State<S>,
  Query(filter): Query<UserInvitationFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserInvitationListItem>, AppError> {
  Ok(UserInvitationService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(uuid): Path<Uuid>) -> Result<GetUserInvitation, AppError> {
  match UserInvitationService::get(uuid).await? {
    Some(user_invitation) => Ok(user_invitation),
    None => Err(AppError::NoData("UserInvitation".to_owned(), uuid.to_string())),
  }
}

async fn create<S>(
  State(_state): State<S>,
  Path(username): Path<String>,
  Json(_body): Json<CreateUserInvitation>,
) -> Result<UserInvitationCreated, AppError> {
  let invitation = UserInvitationService::create(username).await?;

  Ok(UserInvitationCreated {
    uuid: invitation.uuid,
    expired_at: invitation.expired_at,
  })
}

async fn update<S>(
  State(_state): State<S>,
  Path(username): Path<String>,
  Json(_body): Json<UpdateUserInvitation>,
) -> Result<UserInvitationUpdated, AppError> {
  let invitation = UserInvitationService::update(username).await?;

  Ok(UserInvitationUpdated {
    uuid: invitation.uuid,
    expired_at: invitation.expired_at,
  })
}

async fn delete<S>(
  State(_state): State<S>,
  Path(uuid): Path<Uuid>,
) -> Result<UserInvitationDeleted, AppError> {
  UserInvitationService::delete(uuid).await?;
  Ok(UserInvitationDeleted)
}

pub fn new<S>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  let route_list = Router::new().route("/", routing::get(list::<S>)).route_layer(admin_guard!(state));
  let route_get = Router::new().route("/{uuid}", routing::get(get::<S>));
  let route_create = Router::new().route("/", routing::post(create::<S>)).route_layer(admin_guard!(state));
  let route_update =
    Router::new().route("/{uuid}", routing::patch(update::<S>)).route_layer(admin_guard!(state));
  let route_delete =
    Router::new().route("/{uuid}", routing::delete(delete::<S>)).route_layer(admin_guard!(state));

  Router::new()
    .merge(route_list)
    .merge(route_get)
    .merge(route_create)
    .merge(route_update)
    .merge(route_delete)
    .route_layer(user_guard!(state))
}
