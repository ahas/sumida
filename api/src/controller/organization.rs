use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct OrganizationController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> OrganizationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<OrganizationFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<OrganizationListItem>, SumidaError> {
    Ok(OrganizationService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetOrganization, SumidaError> {
    OrganizationService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Organization".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateOrganization>,
  ) -> Result<OrganizationCreated, SumidaError> {
    Ok(OrganizationCreated {
      id: OrganizationService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateOrganization>,
  ) -> Result<OrganizationUpdated, SumidaError> {
    OrganizationService::new(state, session).update(id, body).await?;
    Ok(OrganizationUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<OrganizationDeleted, SumidaError> {
    OrganizationService::new(state, session).delete(id).await?;
    Ok(OrganizationDeleted)
  }
}

impl<S> ApiController<S> for OrganizationController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "organization", read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "organization", create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "organization", update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "organization", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
