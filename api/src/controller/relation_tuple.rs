use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

/// `/relation-tuples` — the record of who holds what (design §9.6).
///
/// Two gates, and they answer with **different error codes**, which is the thing to hold on to when
/// reading a refusal:
///
/// * the route gate (`service_ap!`) asks `service:access_policy#can_<action>` and refuses with
///   `NoServiceAvailable` — `403`, code `1009`. It runs **before** the handler, so it says nothing
///   about the tuple being written.
/// * `Authz::write` / `Authz::revoke` ask `<resource_type>#grant` **on the object named in the
///   request** and refuse with `NoAccessPermission` — `403`, code `1008`.
///
/// Both are `403`, and conflating them is how a test passes with the row scope deleted: the gate
/// refuses first, so a caller who was never granted the service tier never reaches the part under
/// test. Assert the code, not the status.
///
/// `/access-policies` stays mounted beside this, read-only, for as long as Phase 1–2 runs (§9.6).
#[derive(Clone, Debug, Default)]
pub struct RelationTupleController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> RelationTupleController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<RelationTupleFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<RelationTupleListItem>, SumidaError> {
    Ok(RelationTupleService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i64>,
  ) -> Result<RelationTupleListItem, SumidaError> {
    RelationTupleService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("RelationTuple".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateRelationTuple>,
  ) -> Result<RelationTupleCreated, SumidaError> {
    Ok(RelationTupleCreated {
      id: RelationTupleService::new(state, session).create(body).await?,
    })
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i64>,
  ) -> Result<RelationTupleDeleted, SumidaError> {
    RelationTupleService::new(state, session).delete(id).await?;
    Ok(RelationTupleDeleted)
  }
}

impl<S> ApiController<S> for RelationTupleController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    // The namespace is `access_policy`, not a new one: this endpoint replaces that screen, and
    // giving it a namespace of its own would silently take the capability away from everyone who
    // holds it today. `service:access_policy` resolves to the `relation_tuple` definition through
    // the inventory (`schema.sz`'s `namespace access_policy service = relation_tuple`).
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "access_policy", read));
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "access_policy", create));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "access_policy", delete));

    Router::new() //
      .merge(route_read)
      .merge(route_create)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}
