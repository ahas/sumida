use std::marker::PhantomData;

use axum::extract::State;
use axum::{Json, Router, routing};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{ENTITLEMENT_TAG, ErrorBody, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/entitlements";

const CHECK: &str = "/check";

#[derive(Clone, Debug, Default)]
pub struct EntitlementController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> EntitlementController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// Ask whether an action may go ahead, and pay for it in the same breath.
///
/// **This is not a question — it spends.** An active subscription that covers the action and has
/// quota left records one use of it; failing that, the action's cost is taken out of the credit
/// wallet. `via` says which of the two answered and `consumed` says what it cost, which is `0` when
/// a subscription covered it.
///
/// Nothing is spent when the answer is a refusal: `402` means neither a subscription nor the wallet
/// could cover the action, and `403` means the plan does not cover it at all or the subscription is
/// no longer live.
///
/// `idempotencyKey` makes a retry safe: the same key seen twice answers with what the first call
/// did rather than spending again.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CHECK,
  context_path = MOUNT,
  tag = ENTITLEMENT_TAG,
  operation_id = "entitlement_check",
  request_body = RequireActionRequest,
  responses(
    (status = 200, description = "Allowed. Which side paid for it, and what it cost", body = EntitlementOutcome),
    (status = 402, description = "Not covered, and the wallet does not hold enough. Nothing was spent", body = ErrorBody),
    (
      status = 403,
      description = "No live subscription, or one whose plan does not cover this action. Nothing \
                     was spent.",
      body = ErrorBody,
    ),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn check<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<RequireActionRequest>,
) -> Result<EntitlementOutcome, SumidaError> {
  EntitlementService::new(state, session)
    .require_action(&body.action, body.owner_kind, body.owner_id, body.idempotency_key.as_deref())
    .await
}

impl<S: SumidaState> ApiController<S> for EntitlementController<S> {
  fn router(self, state: &S) -> Router<S> {
    Router::new()
      .route(CHECK, routing::post(check::<S>))
      .route_layer(user_guard!(state))
  }
}
