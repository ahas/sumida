use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
use uuid::Uuid;

use super::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_router};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct UserInvitationController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> UserInvitationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<UserInvitationFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserInvitationListItem>, SumidaError> {
    Ok(UserInvitationService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<GetUserInvitation, SumidaError> {
    UserInvitationService::new(state, session)
      .get(uuid)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
    Json(_body): Json<CreateUserInvitation>,
  ) -> Result<UserInvitationCreated, SumidaError> {
    let invitation = UserInvitationService::new(state, session).create(user_id).await?;

    Ok(UserInvitationCreated {
      uuid: invitation.uuid,
      expires_at: invitation.expires_at,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
    Json(_body): Json<UpdateUserInvitation>,
  ) -> Result<UserInvitationUpdated, SumidaError> {
    let invitation = UserInvitationService::new(state, session).update(user_id).await?;

    Ok(UserInvitationUpdated {
      uuid: invitation.uuid,
      expires_at: invitation.expires_at,
    })
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<UserInvitationDeleted, SumidaError> {
    UserInvitationService::new(state, session).delete(uuid).await?;
    Ok(UserInvitationDeleted)
  }
}

impl<S> ApiController<S> for UserInvitationController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{uuid}", routing::get(Self::get))
      .route_layer(service_ap!(state, "user_invitation", read));
    let route_filters = filter_descriptor_router::<S, UserInvitationFilter>(state, "user_invitation");
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "user_invitation", create));
    let route_update = Router::new() //
      .route("/{uuid}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "user_invitation", update));
    let route_delete = Router::new() //
      .route("/{uuid}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "user_invitation", delete));

    Router::new()
      .merge(route_read)
      .merge(route_filters)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
