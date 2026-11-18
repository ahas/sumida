use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, PLAN_TAG, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/membership-plans";

const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct PlanController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> PlanController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// What may be subscribed to or bought, and on what terms.
///
/// Retired plans are left out unless `includeDeleted` asks for them: a plan is retired by a soft
/// delete so that the subscriptions already standing on it keep their terms.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = PLAN_TAG,
  operation_id = "plan_list",
  params(PlanFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of plans, and how many there are behind it", body = List<PlanListItem>),
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
  Query(filter): Query<PlanFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<PlanListItem>, SumidaError> {
  Ok(PlanService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One plan, with the actions it covers.
///
/// `actions` is what the entitlement gate reads: an action listed here is one the plan allows, with
/// its per-period quota and what it costs in credits.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = PLAN_TAG,
  operation_id = "plan_get",
  params(("id" = i32, Path, description = "The plan's identifier")),
  responses(
    (status = 200, description = "The plan and the actions it covers", body = GetPlan),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetPlan, SumidaError> {
  PlanService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("MembershipPlan".to_owned(), id.to_string()))
}

/// Add a plan.
///
/// `code` is the name every other route calls the plan by — a subscription, a checkout and a coupon
/// all name it rather than the numeric id — so it is chosen here and does not change afterwards.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = PLAN_TAG,
  operation_id = "plan_create",
  request_body = CreatePlan,
  responses(
    (status = 201, description = "The plan's identifier and its code", body = PlanCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreatePlan>,
) -> Result<PlanCreated, SumidaError> {
  let model = PlanService::new(state, session).create(body).await?;
  Ok(PlanCreated {
    id:   model.id,
    code: model.code,
  })
}

/// Change a plan. Only the fields sent are touched — except `actions`, which replaces the whole set
/// when it is sent at all.
///
/// Neither `code` nor `kind` nor `scope` can be changed: they are what the subscriptions already
/// standing on this plan were sold under.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = PLAN_TAG,
  operation_id = "plan_update",
  params(("id" = i32, Path, description = "The plan's identifier")),
  request_body = UpdatePlan,
  responses(
    (status = 200, description = "Changed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn update<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<UpdatePlan>,
) -> Result<PlanUpdated, SumidaError> {
  PlanService::new(state, session).update(id, body).await?;
  Ok(PlanUpdated)
}

/// Retire a plan.
///
/// It is a soft delete: the row stays so that the subscriptions standing on it keep their terms, and
/// it stops appearing in the list unless `includeDeleted` asks for it.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = PLAN_TAG,
  operation_id = "plan_delete",
  params(("id" = i32, Path, description = "The plan's identifier")),
  responses(
    (status = 200, description = "Retired. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<PlanDeleted, SumidaError> {
  PlanService::new(state, session).delete(id).await?;
  Ok(PlanDeleted)
}

impl<S: SumidaState> ApiController<S> for PlanController<S> {
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "membership_plan")
      .read(Router::new().route("/", routing::get(list::<S>)).route(ONE, routing::get(get::<S>)))
      .descriptor::<PlanFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}
