use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct PlanController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> PlanController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<PlanFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<PlanListItem>, SumidaError> {
    Ok(PlanService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetPlan, SumidaError> {
    PlanService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("MembershipPlan".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreatePlan>,
  ) -> Result<PlanCreated, SumidaError> {
    let model = PlanService::new(state, session).create(body).await?;
    Ok(PlanCreated {
      id: model.id,
      code: model.code,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdatePlan>,
  ) -> Result<PlanUpdated, SumidaError> {
    PlanService::new(state, session).update(id, body).await?;
    Ok(PlanUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<PlanDeleted, SumidaError> {
    PlanService::new(state, session).delete(id).await?;
    Ok(PlanDeleted)
  }
}

impl<S: SumidaState> ApiController<S> for PlanController<S> {
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "membership_plan", read));
    let create = Router::new().route("/", routing::post(Self::create)).route_layer(service_ap!(
      state,
      "membership_plan",
      create
    ));
    let update = Router::new().route("/{id}", routing::patch(Self::update)).route_layer(service_ap!(
      state,
      "membership_plan",
      update
    ));
    let delete = Router::new().route("/{id}", routing::delete(Self::delete)).route_layer(service_ap!(
      state,
      "membership_plan",
      delete
    ));

    Router::new()
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}
