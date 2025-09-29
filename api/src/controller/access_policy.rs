use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use uuid::Uuid;

use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service::prelude::*};
use crate::{service_ap, user_guard};

use super::ApiController;

#[derive(Clone, Debug, Default)]
pub struct AccessPolicyController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> AccessPolicyController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<AccessPolicyFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<AccessPolicyListItem>, SumidaError> {
    Ok(AccessPolicyService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<GetAccessPolicy, SumidaError> {
    AccessPolicyService::new(state, session)
      .get(uuid)
      .await?
      .ok_or_else(|| SumidaError::NoData("AccessPolicy".to_owned(), uuid.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateAccessPolicy>,
  ) -> Result<AccessPolicyCreated, SumidaError> {
    Ok(AccessPolicyCreated {
      uuid: AccessPolicyService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
    Json(body): Json<UpdateAccessPolicy>,
  ) -> Result<AccessPolicyUpdated, SumidaError> {
    AccessPolicyService::new(state, session).update(uuid, body).await?;
    Ok(AccessPolicyUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<AccessPolicyDeleted, SumidaError> {
    AccessPolicyService::new(state, session).delete(uuid).await?;
    Ok(AccessPolicyDeleted)
  }
}

impl<S> ApiController<S> for AccessPolicyController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{uuid}", routing::get(Self::get))
      .route_layer(service_ap!(state, "access_policy", read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "access_policy", create));
    let route_update = Router::new() //
      .route("/{uuid}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "access_policy", update));
    let route_delete = Router::new() //
      .route("/{uuid}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "access_policy", delete));

    Router::new() //
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
