use std::marker::PhantomData;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{Json, Router, routing};
use infra::payment::WebhookHeaders;

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

#[derive(Clone, Debug, Default)]
pub struct PaymentController<S: SumidaState>(PhantomData<S>);

fn header(headers: &HeaderMap, name: &str) -> String {
  headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
}

impl<S: SumidaState> PaymentController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  /// Create a pending purchase; returns the order id to register with the PG.
  async fn checkout(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CheckoutRequest>,
  ) -> Result<CheckoutResult, SumidaError> {
    PaymentService::new(state, session).checkout(body).await
  }

  /// Confirm a paid purchase (server-side verification + activation).
  async fn confirm(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<ConfirmRequest>,
  ) -> Result<ConfirmResult, SumidaError> {
    PaymentService::new(state, session).confirm(body).await
  }

  async fn refund(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<PaymentRefundRequest>,
  ) -> Result<PaymentRefundResult, SumidaError> {
    PaymentService::new(state, session)
      .refund(&body.purchase_code, body.reason.as_deref())
      .await
  }

  /// PG webhook — public, authenticated by Standard-Webhooks signature.
  async fn webhook(
    State(state): State<S>,
    headers: HeaderMap,
    body: Bytes,
  ) -> Result<WebhookAck, SumidaError> {
    let wh = WebhookHeaders {
      id: header(&headers, "webhook-id"),
      timestamp: header(&headers, "webhook-timestamp"),
      signature: header(&headers, "webhook-signature"),
    };
    PaymentService::system(state).handle_webhook(wh, body.as_ref()).await?;
    Ok(WebhookAck { received: true })
  }
}

impl<S: SumidaState> ApiController<S> for PaymentController<S> {
  fn router(self, state: &S) -> Router<S> {
    let guarded = Router::new()
      .route("/checkout", routing::post(Self::checkout))
      .route("/confirm", routing::post(Self::confirm))
      .route("/refund", routing::post(Self::refund))
      .route_layer(user_guard!(state));

    // The webhook is unauthenticated at the session layer; it is verified by the
    // provider's signature inside the handler.
    let public = Router::new().route("/webhook", routing::post(Self::webhook));

    Router::new().merge(guarded).merge(public)
  }
}
