use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use uuid::Uuid;

use crate::user_guard;
use crate::util::pagination::{List, Pagination};
use crate::{service::prelude::*, service_ap};

use super::ApiController;

#[derive(Clone, Debug, Default)]
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
    session: UserSession,
    State(_state): State<S>,
    Query(filter): Query<UserInvitationFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserInvitationListItem>, SumidaError> {
    Ok(UserInvitationService::new(session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(_state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<GetUserInvitation, SumidaError> {
    UserInvitationService::new(session)
      .get(uuid)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
  }

  async fn create(
    session: UserSession,
    State(_state): State<S>,
    Path(user_id): Path<i32>,
    Json(_body): Json<CreateUserInvitation>,
  ) -> Result<UserInvitationCreated, SumidaError> {
    let invitation = UserInvitationService::new(session).create(user_id).await?;

    Ok(UserInvitationCreated {
      uuid: invitation.uuid,
      expires_at: invitation.expires_at,
    })
  }

  async fn update(
    session: UserSession,
    State(_state): State<S>,
    Path(user_id): Path<i32>,
    Json(_body): Json<UpdateUserInvitation>,
  ) -> Result<UserInvitationUpdated, SumidaError> {
    let invitation = UserInvitationService::new(session).update(user_id).await?;

    Ok(UserInvitationUpdated {
      uuid: invitation.uuid,
      expires_at: invitation.expires_at,
    })
  }

  async fn delete(
    session: UserSession,
    State(_state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<UserInvitationDeleted, SumidaError> {
    UserInvitationService::new(session).delete(uuid).await?;
    Ok(UserInvitationDeleted)
  }
}

impl<S> ApiController<S> for UserInvitationController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{uuid}", routing::get(Self::get))
      .route_layer(service_ap!("user_invitation", read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!("user_invitation", create));
    let route_update = Router::new() //
      .route("/{uuid}", routing::patch(Self::update))
      .route_layer(service_ap!("user_invitation", update));
    let route_delete = Router::new() //
      .route("/{uuid}", routing::delete(Self::delete))
      .route_layer(service_ap!("user_invitation", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
