use std::marker::PhantomData;

use axum::extract::{Query, State};
use axum::{Router, routing};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{ErrorBody, PURCHASE_TAG, Refused};
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_route};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/membership-purchases";

#[derive(Clone, Debug, Default)]
pub struct PurchaseController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> PurchaseController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// What was bought, and what it came to.
///
/// A row is written for every charge this subsystem records — a checkout, a renewal, and the
/// difference settled when a subscription changes plan mid-period — so it is the read side of
/// billing rather than the customer's receipts alone. `code` is the same string a checkout hands to
/// the payment provider as its order id.
///
/// `amount` is what is charged; `subtotal`, `discountAmount` and `taxAmount` are what it is made of.
///
/// Being signed in is all the route itself asks for. Which rows come back is settled inside the
/// handler: an owner sees their own, and an organization's administrator sees that organization's.
///
/// There is no route here that writes one. A purchase appears because a payment was taken.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = PURCHASE_TAG,
  operation_id = "purchase_list",
  params(PurchaseFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of purchases, and how many there are behind it", body = List<PurchaseListItem>),
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
  Query(filter): Query<PurchaseFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<PurchaseListItem>, SumidaError> {
  Ok(PurchaseService::new(state, session).list(filter, conditions, pagination).await?.into())
}

impl<S: SumidaState> ApiController<S> for PurchaseController<S> {
  fn router(self, state: &S) -> Router<S> {
    // 서술은 목록과 같은 게이트를 받아야 하고, 이 목록이 서 있는 것은 아래 `user_guard!` 하나뿐
    // 이므로 게이트를 스스로 달지 않는 라우트를 그 안에 합친다.
    Router::new()
      .route("/", routing::get(list::<S>))
      .merge(filter_descriptor_route::<S, PurchaseFilter>())
      .route_layer(user_guard!(state))
  }
}
