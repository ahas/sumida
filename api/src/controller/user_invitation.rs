use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use uuid::Uuid;

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{admin_guard, user_guard};

use super::ApiController;

pub struct UserInvitationController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> UserInvitationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    _user_ctx: UserContext,
    State(_state): State<S>,
    Query(filter): Query<UserInvitationFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserInvitationListItem>, SumidaError> {
    Ok(UserInvitationService::list(filter, pagination).await?.into())
  }

  async fn get(State(_state): State<S>, Path(uuid): Path<Uuid>) -> Result<GetUserInvitation, SumidaError> {
    match UserInvitationService::get(uuid).await? {
      Some(user_invitation) => Ok(user_invitation),
      None => Err(SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string())),
    }
  }

  async fn create(
    State(_state): State<S>,
    Path(user_id): Path<i32>,
    Json(_body): Json<CreateUserInvitation>,
  ) -> Result<UserInvitationCreated, SumidaError> {
    let invitation = UserInvitationService::create(user_id).await?;

    Ok(UserInvitationCreated {
      uuid: invitation.uuid,
      expires_at: invitation.expires_at,
    })
  }

  async fn update(
    State(_state): State<S>,
    Path(user_id): Path<i32>,
    Json(_body): Json<UpdateUserInvitation>,
  ) -> Result<UserInvitationUpdated, SumidaError> {
    let invitation = UserInvitationService::update(user_id).await?;

    Ok(UserInvitationUpdated {
      uuid: invitation.uuid,
      expires_at: invitation.expires_at,
    })
  }

  async fn delete(
    State(_state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<UserInvitationDeleted, SumidaError> {
    UserInvitationService::delete(uuid).await?;
    Ok(UserInvitationDeleted)
  }
}

impl<S> ApiController<S> for UserInvitationController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_list = Router::new() //
      .route("/", routing::get(Self::list))
      .route_layer(admin_guard!(state));
    let route_get = Router::new() //
      .route("/{uuid}", routing::get(Self::get));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(admin_guard!(state));
    let route_update = Router::new() //
      .route("/{uuid}", routing::patch(Self::update))
      .route_layer(admin_guard!(state));
    let route_delete = Router::new() //
      .route("/{uuid}", routing::delete(Self::delete))
      .route_layer(admin_guard!(state));

    Router::new()
      .merge(route_list)
      .merge(route_get)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
