use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_route};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

#[derive(Clone, Debug, Default)]
pub struct SubscriptionController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> SubscriptionController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<SubscriptionFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<SubscriptionListItem>, SumidaError> {
    Ok(
      SubscriptionService::new(state, session)
        .list(filter, conditions, pagination)
        .await?
        .into(),
    )
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetSubscription, SumidaError> {
    SubscriptionService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Subscription".to_owned(), id.to_string()))
  }

  async fn subscribe(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateSubscription>,
  ) -> Result<SubscriptionCreated, SumidaError> {
    SubscriptionService::new(state, session).subscribe(body).await
  }

  async fn cancel(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Query(query): Query<CancelQuery>,
  ) -> Result<SubscriptionUpdated, SumidaError> {
    SubscriptionService::new(state, session).cancel(id, query.at_period_end).await?;
    Ok(SubscriptionUpdated)
  }

  async fn renew(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<SubscriptionUpdated, SumidaError> {
    SubscriptionService::new(state, session).renew(id).await?;
    Ok(SubscriptionUpdated)
  }

  async fn set_auto_renew(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<SetAutoRenew>,
  ) -> Result<SubscriptionUpdated, SumidaError> {
    SubscriptionService::new(state, session).set_auto_renew(id, body).await?;
    Ok(SubscriptionUpdated)
  }

  async fn reactivate(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<SubscriptionUpdated, SumidaError> {
    SubscriptionService::new(state, session).reactivate(id).await?;
    Ok(SubscriptionUpdated)
  }

  async fn change_plan(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<ChangePlan>,
  ) -> Result<ProrationResult, SumidaError> {
    SubscriptionService::new(state, session).change_plan(id, body).await
  }
}

impl<S: SumidaState> ApiController<S> for SubscriptionController<S> {
  fn router(self, state: &S) -> Router<S> {
    // Authorization is enforced inside the service (owner / org-admin via ReBAC),
    // so the route layer only requires an authenticated user.
    // 서술은 목록과 같은 게이트를 받아야 하고, 이 목록이 서 있는 것은 아래 `user_guard!` 하나뿐
    // 이므로 게이트를 스스로 달지 않는 라우트를 그 안에 합친다.
    Router::new()
      .merge(filter_descriptor_route::<S, SubscriptionFilter>())
      .route("/", routing::get(Self::list).post(Self::subscribe))
      .route("/{id}", routing::get(Self::get))
      .route("/{id}/cancel", routing::post(Self::cancel))
      .route("/{id}/renew", routing::post(Self::renew))
      .route("/{id}/auto-renew", routing::post(Self::set_auto_renew))
      .route("/{id}/reactivate", routing::post(Self::reactivate))
      .route("/{id}/change-plan", routing::post(Self::change_plan))
      .route_layer(user_guard!(state))
  }
}
