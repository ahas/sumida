use std::marker::PhantomData;

use axum::extract::{Path, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{ENTITLEMENT_TAG, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/entitlements";

const CHECK: &str = "/check";
const RESERVE: &str = "/reserve";
const EXTEND: &str = "/reservations/{id}/extend";
const CAPTURE: &str = "/reservations/{id}/capture";
const RELEASE: &str = "/reservations/{id}/release";

/// Who may move a quota hold, and what a repeated request does.
#[cfg(feature = "openapi")]
const QUOTA_STEP: &str = "Being signed in is all the route itself asks for. The hold's subscription \
is reached by its owner, a member of the organization that owns it, or a platform administrator. \
`idempotencyKey` makes a retry safe: the same key seen twice on the subscription answers with the \
hold as the first call left it instead of moving it again.";

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
/// quota left records one use of it, and with `amount` that amount against its amount quota;
/// failing that, `amount` — or the action's cost when none is given — is taken out of the credit
/// wallet. `via` says which of the two answered and `consumed` says what it cost in credits, which
/// is `0` when a subscription covered it.
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
    .require_action(
      &body.action,
      body.owner_kind,
      body.owner_id,
      body.amount,
      body.idempotency_key.as_deref(),
    )
    .await
}

/// Hold an amount of the quota the owner's live subscription carries for an action.
///
/// The hold comes out of the period's room — its amount limit less what was used and what live
/// holds take — until it is captured, released or expires, after `expiresInSecs` or the configured
/// lifetime. An action covered with no amount limit always has room.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = RESERVE,
  context_path = MOUNT,
  tag = ENTITLEMENT_TAG,
  operation_id = "entitlement_reserve",
  description = QUOTA_STEP,
  request_body = ReserveQuota,
  responses(
    (status = 200, description = "The hold and the period it holds against", body = QuotaReservationResult),
    (status = 402, description = "The period's room is short. Nothing was held", body = ErrorBody),
    (status = 403, description = "No live subscription covers this action. Nothing was held", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn reserve<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<ReserveQuota>,
) -> Result<QuotaReservationResult, SumidaError> {
  QuotaService::new(state, session).reserve(body).await
}

/// Grow a live quota hold by `amount` and restart its expiry.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = EXTEND,
  context_path = MOUNT,
  tag = ENTITLEMENT_TAG,
  operation_id = "entitlement_extend",
  description = QUOTA_STEP,
  params(("id" = i32, Path, description = "The hold's identifier")),
  request_body = ExtendReservation,
  responses(
    (status = 200, description = "The hold and the period it holds against", body = QuotaReservationResult),
    (status = 402, description = "The period's room is short. Nothing changed", body = ErrorBody),
    (status = 409, description = "The hold has expired", body = ErrorBody),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn extend<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<ExtendReservation>,
) -> Result<QuotaReservationResult, SumidaError> {
  QuotaService::new(state, session).extend(id, body).await
}

/// Record a quota hold as usage at the amount actually used, which may be more than the hold.
///
/// The usage lands in the period the hold was taken in. Past the period's limit it goes up to
/// `overageLimit` further and is refused with `402` beyond, the hold staying held; the excess is
/// not carried into the next period. An `overageLimit` above zero needs `edit_subscription` on the
/// owning organization, or a platform administrator on a personal subscription. The body may be
/// left out, which captures the hold's own amount.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CAPTURE,
  context_path = MOUNT,
  tag = ENTITLEMENT_TAG,
  operation_id = "entitlement_capture",
  description = QUOTA_STEP,
  params(("id" = i32, Path, description = "The hold's identifier")),
  request_body = Option<CaptureQuota>,
  responses(
    (status = 200, description = "The captured hold and its period's usage", body = QuotaReservationResult),
    (status = 402, description = "The usage would pass the limit by more than `overageLimit`. Nothing changed", body = ErrorBody),
    (status = 409, description = "The hold has expired", body = ErrorBody),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn capture<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  body: Option<Json<CaptureQuota>>,
) -> Result<QuotaReservationResult, SumidaError> {
  let body = body.map(|Json(b)| b).unwrap_or_default();
  QuotaService::new(state, session).capture(id, body).await
}

/// Drop a quota hold, expired or not. Its amount goes back to the period's room.
///
/// The body may be left out.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = RELEASE,
  context_path = MOUNT,
  tag = ENTITLEMENT_TAG,
  operation_id = "entitlement_release",
  description = QUOTA_STEP,
  params(("id" = i32, Path, description = "The hold's identifier")),
  request_body = Option<ReleaseReservation>,
  responses(
    (status = 200, description = "The released hold and its period", body = QuotaReservationResult),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn release<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  body: Option<Json<ReleaseReservation>>,
) -> Result<QuotaReservationResult, SumidaError> {
  let body = body.map(|Json(b)| b).unwrap_or_default();
  QuotaService::new(state, session).release(id, body).await
}

impl<S: SumidaState> ApiController<S> for EntitlementController<S> {
  fn router(self, state: &S) -> Router<S> {
    Router::new()
      .route(CHECK, routing::post(check::<S>))
      .route(RESERVE, routing::post(reserve::<S>))
      .route(EXTEND, routing::post(extend::<S>))
      .route(CAPTURE, routing::post(capture::<S>))
      .route(RELEASE, routing::post(release::<S>))
      .route_layer(user_guard!(state))
  }
}
