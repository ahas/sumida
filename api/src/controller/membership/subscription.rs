use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, Refused, SUBSCRIPTION_TAG, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_route};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/subscriptions";

const ONE: &str = "/{id}";
const CANCEL: &str = "/{id}/cancel";
const RENEW: &str = "/{id}/renew";
const AUTO_RENEW: &str = "/{id}/auto-renew";
const REACTIVATE: &str = "/{id}/reactivate";
const CHANGE_PLAN: &str = "/{id}/change-plan";

/// Who these routes let through, repeated on each of them because a caller reading one operation
/// has no reason to look at the others.
#[cfg(feature = "openapi")]
const OWNERSHIP: &str = "Being signed in is all the route itself asks for. Whether this particular \
subscription may be read or acted on is settled inside the handler: its owner may, and so may an \
administrator of the organization that owns it. Anyone else is refused after the request was \
accepted, not before.";

#[derive(Clone, Debug, Default)]
pub struct SubscriptionController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> SubscriptionController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// The subscriptions this caller may see.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_list",
  description = OWNERSHIP,
  params(SubscriptionFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of subscriptions, and how many there are behind it", body = List<SubscriptionListItem>),
    (
      status = 400,
      description = "A query-string value is not of the shape the reader takes (plain text), or \
                     the condition tree names a key, an operator or a size this list does not \
                     accept (error body).",
      content((String = "text/plain"), (ErrorBody = "application/json")),
    ),
    Refused,
  ),
))]
async fn list<S: SumidaState>(
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

/// One subscription, with the actions its plan covers as they stood when the period began.
///
/// `allowedActions` is that snapshot — an object keyed by action, whose value is the per-period
/// quota or null for no limit. It is what the entitlement gate reads, so a plan edited mid-period
/// does not change what this period allows.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_get",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The subscription's identifier")),
  responses(
    (status = 200, description = "The subscription and its allowance", body = GetSubscription),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetSubscription, SumidaError> {
  SubscriptionService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("Subscription".to_owned(), id.to_string()))
}

/// Start a subscription.
///
/// This route does not take a payment. It records the subscription and starts its first period — a
/// free trial where the plan has one — so a plan that has to be paid for goes through
/// `POST /payments/checkout` instead.
///
/// `idempotencyKey` makes a retry safe: the same key twice answers with the subscription the first
/// call made rather than starting a second one. `autoRenew` needs a `billingKey`, which is what the
/// payment provider will be charged against when the period ends.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_subscribe",
  description = OWNERSHIP,
  request_body = CreateSubscription,
  responses(
    (status = 201, description = "The subscription's identifier and its code", body = SubscriptionCreated),
    (status = 404, description = "No plan is stored under that code", body = ErrorBody),
    (status = 409, description = "This owner already has an active subscription to that plan", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn subscribe<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateSubscription>,
) -> Result<SubscriptionCreated, SumidaError> {
  SubscriptionService::new(state, session).subscribe(body).await
}

/// Cancel a subscription.
///
/// `atPeriodEnd=true` lets the period it has already been paid for run out and stops it renewing;
/// left out or false, it ends now. The choice is a query parameter, not a body.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CANCEL,
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_cancel",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The subscription's identifier"), CancelQuery),
  responses(
    (status = 200, description = "Cancelled. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn cancel<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Query(query): Query<CancelQuery>,
) -> Result<SubscriptionUpdated, SumidaError> {
  SubscriptionService::new(state, session).cancel(id, query.at_period_end).await?;
  Ok(SubscriptionUpdated)
}

/// Begin the next period at once, without waiting for the current one to run out.
///
/// The body is not read: the path says which subscription, and there is nothing else to decide.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = RENEW,
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_renew",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The subscription's identifier")),
  responses(
    (status = 200, description = "Renewed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn renew<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<SubscriptionUpdated, SumidaError> {
  SubscriptionService::new(state, session).renew(id).await?;
  Ok(SubscriptionUpdated)
}

/// Turn automatic renewal on or off.
///
/// Turning it on needs a `billingKey` the provider will accept when the period ends; without one
/// the period simply lapses.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = AUTO_RENEW,
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_set_auto_renew",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The subscription's identifier")),
  request_body = SetAutoRenew,
  responses(
    (status = 200, description = "Set. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn set_auto_renew<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<SetAutoRenew>,
) -> Result<SubscriptionUpdated, SumidaError> {
  SubscriptionService::new(state, session).set_auto_renew(id, body).await?;
  Ok(SubscriptionUpdated)
}

/// Take back a cancellation that has not taken effect yet, or bring a lapsed subscription back.
///
/// The body is not read.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = REACTIVATE,
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_reactivate",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The subscription's identifier")),
  responses(
    (status = 200, description = "Revived. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn reactivate<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<SubscriptionUpdated, SumidaError> {
  SubscriptionService::new(state, session).reactivate(id).await?;
  Ok(SubscriptionUpdated)
}

/// Move a subscription onto another plan in the middle of a period, and settle the difference.
///
/// The answer says what the two sides came to: what the unused part of the old plan is worth back,
/// what the new plan costs for the rest of the period, and the net of the two. A negative net is
/// owed to the customer rather than by them. A purchase row is written for the difference either
/// way, and `purchaseCode` names it.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CHANGE_PLAN,
  context_path = MOUNT,
  tag = SUBSCRIPTION_TAG,
  operation_id = "subscription_change_plan",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The subscription's identifier")),
  request_body = ChangePlan,
  responses(
    (status = 200, description = "What the change came to, and the purchase written for it", body = ProrationResult),
    (status = 404, description = "No plan is stored under that code", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn change_plan<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<ChangePlan>,
) -> Result<ProrationResult, SumidaError> {
  SubscriptionService::new(state, session).change_plan(id, body).await
}

impl<S: SumidaState> ApiController<S> for SubscriptionController<S> {
  fn router(self, state: &S) -> Router<S> {
    // Authorization is enforced inside the service (owner / org-admin via ReBAC),
    // so the route layer only requires an authenticated user.
    // 서술은 목록과 같은 게이트를 받아야 하고, 이 목록이 서 있는 것은 아래 `user_guard!` 하나뿐
    // 이므로 게이트를 스스로 달지 않는 라우트를 그 안에 합친다.
    Router::new()
      .merge(filter_descriptor_route::<S, SubscriptionFilter>())
      .route("/", routing::get(list::<S>))
      .route("/", routing::post(subscribe::<S>))
      .route(ONE, routing::get(get::<S>))
      .route(CANCEL, routing::post(cancel::<S>))
      .route(RENEW, routing::post(renew::<S>))
      .route(AUTO_RENEW, routing::post(set_auto_renew::<S>))
      .route(REACTIVATE, routing::post(reactivate::<S>))
      .route(CHANGE_PLAN, routing::post(change_plan::<S>))
      .route_layer(user_guard!(state))
  }
}
