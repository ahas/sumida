use std::marker::PhantomData;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{Json, Router, routing};
use infra::payment::WebhookHeaders;

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{ErrorBody, PAYMENT_TAG, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/payments";

const CHECKOUT: &str = "/checkout";
const CONFIRM: &str = "/confirm";
const REFUND: &str = "/refund";
const WEBHOOK: &str = "/webhook";

#[derive(Clone, Debug, Default)]
pub struct PaymentController<S: SumidaState>(PhantomData<S>);

fn header(headers: &HeaderMap, name: &str) -> String {
  headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
}

impl<S: SumidaState> PaymentController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// Price a purchase and open it, before any money moves.
///
/// It writes a pending purchase and answers with what to charge and what to charge it against.
/// `purchaseCode` is the order id to hand the payment provider — for PortOne V2 it is the
/// `paymentId` — and `provider` names which provider this deployment is wired to.
///
/// Coupons are applied here, not at confirmation: `couponCode` for one, `couponCodes` for several,
/// and several are only accepted where every one of them is stackable. What comes back is already
/// net of them.
///
/// Nothing is subscribed and no credits are granted yet. That happens at confirmation.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CHECKOUT,
  context_path = MOUNT,
  tag = PAYMENT_TAG,
  operation_id = "payment_checkout",
  request_body = CheckoutRequest,
  responses(
    (status = 201, description = "The order id, what to charge, and what it is made of", body = CheckoutResult),
    (status = 404, description = "No plan is stored under that code", body = ErrorBody),
    (status = 503, description = "This deployment has no payment provider configured", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn checkout<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CheckoutRequest>,
) -> Result<CheckoutResult, SumidaError> {
  PaymentService::new(state, session).checkout(body).await
}

/// Finish a purchase once the payment provider says it was paid.
///
/// The amount is verified against the provider from the server before anything is handed over, so a
/// client claiming a payment that did not happen is refused. What is handed over depends on the
/// plan: a subscription starts, or credits arrive, and `subscriptionId` / `walletId` say which.
///
/// This is also what the provider's own callback ends up doing, so a client that misses the answer
/// need not treat the purchase as lost.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CONFIRM,
  context_path = MOUNT,
  tag = PAYMENT_TAG,
  operation_id = "payment_confirm",
  request_body = ConfirmRequest,
  responses(
    (status = 200, description = "The purchase's state, and what it turned into", body = ConfirmResult),
    (status = 400, description = "The provider reports a different amount than the purchase records", body = ErrorBody),
    (status = 402, description = "The provider does not report this payment as completed", body = ErrorBody),
    (status = 404, description = "No purchase is stored under that code", body = ErrorBody),
    (status = 502, description = "The payment provider could not be reached, or answered with an error", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn confirm<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<ConfirmRequest>,
) -> Result<ConfirmResult, SumidaError> {
  PaymentService::new(state, session).confirm(body).await
}

/// Give a purchase back.
///
/// The charge is cancelled with the payment provider and what it granted is undone here.
/// `cancelledAmount` is what the provider actually reversed.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = REFUND,
  context_path = MOUNT,
  tag = PAYMENT_TAG,
  operation_id = "payment_refund",
  request_body = PaymentRefundRequest,
  responses(
    (status = 200, description = "The purchase's state and what was reversed", body = PaymentRefundResult),
    (status = 404, description = "No purchase is stored under that code", body = ErrorBody),
    (status = 502, description = "The payment provider could not be reached, or refused the reversal", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn refund<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<PaymentRefundRequest>,
) -> Result<PaymentRefundResult, SumidaError> {
  PaymentService::new(state, session)
    .refund(&body.purchase_code, body.reason.as_deref())
    .await
}

/// The payment provider's callback. **The one route in this fragment that takes no credentials.**
///
/// It is not open, though: the request is authenticated by its Standard-Webhooks signature, carried
/// in `webhook-id`, `webhook-timestamp` and `webhook-signature`, and a body that does not verify
/// against the deployment's shared secret is refused with `400`. Nothing about the session, the
/// bearer token or an api key is read.
///
/// The body is whatever the provider sends and is not described here — it belongs to the provider,
/// changes with the provider, and is verified as bytes rather than parsed into a shape of ours.
/// What comes back is an acknowledgement, and it means the callback was accepted rather than that a
/// purchase changed.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = WEBHOOK,
  context_path = MOUNT,
  tag = PAYMENT_TAG,
  operation_id = "payment_webhook",
  security(()),
  request_body(
    content = String,
    content_type = "application/json",
    description = "The provider's own event body, verified as bytes against the signature headers.",
  ),
  params(
    ("webhook-id" = String, Header, description = "The provider's event identifier"),
    ("webhook-timestamp" = String, Header, description = "When the provider signed it, in seconds since the epoch"),
    ("webhook-signature" = String, Header, description = "The signature over the id, the timestamp and the body"),
  ),
  responses(
    (status = 200, description = "Accepted", body = WebhookAck),
    (status = 400, description = "The signature does not verify, or the event could not be acted on", body = ErrorBody),
    (status = 503, description = "This deployment has no payment provider configured", body = ErrorBody),
  ),
))]
async fn webhook<S: SumidaState>(
  State(state): State<S>,
  headers: HeaderMap,
  body: Bytes,
) -> Result<WebhookAck, SumidaError> {
  let wh = WebhookHeaders {
    id:        header(&headers, "webhook-id"),
    timestamp: header(&headers, "webhook-timestamp"),
    signature: header(&headers, "webhook-signature"),
  };
  PaymentService::system(state).handle_webhook(wh, body.as_ref()).await?;
  Ok(WebhookAck { received: true })
}

impl<S: SumidaState> ApiController<S> for PaymentController<S> {
  fn router(self, state: &S) -> Router<S> {
    let guarded = Router::new()
      .route(CHECKOUT, routing::post(checkout::<S>))
      .route(CONFIRM, routing::post(confirm::<S>))
      .route(REFUND, routing::post(refund::<S>))
      .route_layer(user_guard!(state));

    // The webhook is unauthenticated at the session layer; it is verified by the
    // provider's signature inside the handler.
    let public = Router::new().route(WEBHOOK, routing::post(webhook::<S>));

    Router::new().merge(guarded).merge(public)
  }
}
