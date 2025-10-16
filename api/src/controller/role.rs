use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct RoleController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S> RoleController<S>
where
  S: SumidaState,
{
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<RoleFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<RoleListItem>, SumidaError> {
    Ok(RoleService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetRole, SumidaError> {
    RoleService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Role".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateRole>,
  ) -> Result<RoleCreated, SumidaError> {
    Ok(RoleCreated {
      id: RoleService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateRole>,
  ) -> Result<RoleUpdated, SumidaError> {
    RoleService::new(state, session).update(id, body).await?;
    Ok(RoleUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<RoleDeleted, SumidaError> {
    RoleService::new(state, session).delete(id).await?;
    Ok(RoleDeleted)
  }
}

impl<S> ApiController<S> for RoleController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "role", read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "role", create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "role", update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "role", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
