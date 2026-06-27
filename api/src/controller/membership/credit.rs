use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

#[derive(Clone, Debug, Default)]
pub struct CreditController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> CreditController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn balance(
    session: UserSession,
    State(state): State<S>,
    Query(owner): Query<OwnerQuery>,
  ) -> Result<WalletBalance, SumidaError> {
    CreditService::new(state, session).balance(owner.owner_kind, owner.owner_id).await
  }

  async fn grant(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<GrantCredit>,
  ) -> Result<GrantResult, SumidaError> {
    CreditService::new(state, session).grant(body).await
  }

  async fn consume(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<ConsumeCredit>,
  ) -> Result<ConsumeResult, SumidaError> {
    CreditService::new(state, session).consume(body).await
  }

  async fn refund(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<RefundCredit>,
  ) -> Result<ConsumeResult, SumidaError> {
    CreditService::new(state, session)
      .refund(body.owner_kind, body.owner_id, body.amount, body.reference_code, body.idempotency_key)
      .await
  }

  async fn transactions(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<CreditTxFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<CreditTxItem>, SumidaError> {
    Ok(CreditService::new(state, session).transactions(filter, pagination).await?.into())
  }

  async fn reserve(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<ReserveCredit>,
  ) -> Result<ReservationResult, SumidaError> {
    CreditService::new(state, session).reserve(body).await
  }

  async fn capture(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<ConsumeResult, SumidaError> {
    CreditService::new(state, session).capture(id).await
  }

  async fn release(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<ReservationResult, SumidaError> {
    CreditService::new(state, session).release(id).await
  }
}

impl<S: SumidaState> ApiController<S> for CreditController<S> {
  fn router(self, state: &S) -> Router<S> {
    // Owner / org-admin authorization is enforced inside the service.
    Router::new()
      .route("/balance", routing::get(Self::balance))
      .route("/transactions", routing::get(Self::transactions))
      .route("/grant", routing::post(Self::grant))
      .route("/consume", routing::post(Self::consume))
      .route("/refund", routing::post(Self::refund))
      .route("/reserve", routing::post(Self::reserve))
      .route("/reservations/{id}/capture", routing::post(Self::capture))
      .route("/reservations/{id}/release", routing::post(Self::release))
      .route_layer(user_guard!(state))
  }
}
