use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
#[cfg(feature = "openapi")]
use crate::openapi::{COUPON_TAG, EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/coupons";

const ONE: &str = "/{id}";
const GRANT: &str = "/{id}/grant";
const GRANT_BULK: &str = "/{id}/grant-bulk";
const GRANTS: &str = "/{id}/grants";
const MINE: &str = "/mine";
const REDEEM: &str = "/redeem";

/// What the two halves of this resource ask for. Repeated on the routes it applies to, because a
/// caller reading one operation has no reason to look at the others.
#[cfg(feature = "openapi")]
const HOLDER: &str = "This is the holder's side of the resource and it takes no coupon-administration \
capability — being signed in is enough. Requiring one would stop an ordinary account from using \
what was issued to it.";

#[derive(Clone, Debug, Default)]
pub struct CouponController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> CouponController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// The coupons this installation has issued.
///
/// A coupon carries exactly one reward: `percentOff` takes a share off a checkout, `grantCredits`
/// puts credits in a wallet, `freePlanCode` gives a period of a plan. Two of the three are never set
/// at once.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_list",
  params(CouponFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of coupons, and how many there are behind it", body = List<CouponItem>),
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
  Query(filter): Query<CouponFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<CouponItem>, SumidaError> {
  Ok(CouponService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// Mint a coupon.
///
/// `code` is what a customer types, so it is chosen here rather than assigned. Exactly one of
/// `percentOff`, `grantCredits` and `freePlanCode` must be set; naming none or several is refused.
///
/// `requiresGrant` decides who may spend it: false and the code works for anyone who has it, true
/// and only the owners it was handed to. `stackable` decides whether it may sit beside another
/// coupon in one checkout.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_create",
  request_body = CreateCoupon,
  responses(
    (status = 201, description = "The coupon's identifier and its code", body = CouponCreated),
    (status = 400, description = "No reward was named, or more than one was", body = ErrorBody),
    (status = 409, description = "That code is already in use", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateCoupon>,
) -> Result<CouponCreated, SumidaError> {
  CouponService::new(state, session).create(body).await
}

/// Change a coupon's terms — how long it lasts, how often it may be spent, whether it still works.
///
/// The reward cannot be changed. What a coupon gives is what it was minted to give, and the
/// redemptions already recorded were made against it.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_update",
  params(("id" = i32, Path, description = "The coupon's identifier")),
  request_body = UpdateCoupon,
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
  Json(body): Json<UpdateCoupon>,
) -> Result<CouponUpdated, SumidaError> {
  CouponService::new(state, session).update(id, body).await
}

/// Remove a coupon. It stops being redeemable; what it already gave out stays given.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_delete",
  params(("id" = i32, Path, description = "The coupon's identifier")),
  responses(
    (status = 200, description = "Removed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<CouponDeleted, SumidaError> {
  CouponService::new(state, session).delete(id).await
}

/// Hand a coupon to one owner.
///
/// Handing it out twice is not an error and does not issue a second one. An owner who has already
/// redeemed it is not handed it again — revoke first.
///
/// Handing out and taking back are grouped with editing rather than with creating, because both
/// change the coupon rather than make one.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = GRANT,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_grant",
  params(("id" = i32, Path, description = "The coupon's identifier")),
  request_body = GrantCoupon,
  responses(
    (status = 201, description = "Handed out. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn grant<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<GrantCoupon>,
) -> Result<CouponGranted, SumidaError> {
  CouponService::new(state, session).grant_to(id, body).await
}

/// Take a coupon back from one owner.
///
/// Which owner is named in the **query string**, not in a body — the same two keys the handing-out
/// body carries.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = GRANT,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_revoke",
  params(("id" = i32, Path, description = "The coupon's identifier"), GrantCoupon),
  responses(
    (status = 200, description = "Taken back. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn revoke<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Query(owner): Query<GrantCoupon>,
) -> Result<CouponRevoked, SumidaError> {
  CouponService::new(state, session).revoke(id, owner).await
}

/// Hand a coupon to many owners at once.
///
/// The answer counts what happened rather than failing on the first owner it could not reach:
/// `granted` were issued, `skipped` already held one, and `conflicts` had already redeemed it and
/// were left alone.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = GRANT_BULK,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_grant_bulk",
  params(("id" = i32, Path, description = "The coupon's identifier")),
  request_body = BulkGrantCoupon,
  responses(
    (status = 200, description = "How many were issued, skipped and left alone", body = BulkGrantResult),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn grant_bulk<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<BulkGrantCoupon>,
) -> Result<BulkGrantResult, SumidaError> {
  CouponService::new(state, session).grant_bulk(id, body).await
}

/// Who holds a coupon, and which of them have spent it.
///
/// Paging applies; there is no narrowing beyond the coupon in the path.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = GRANTS,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_grants",
  params(("id" = i32, Path, description = "The coupon's identifier"), Pagination),
  responses(
    (status = 200, description = "One page of holders, and how many there are behind it", body = List<CouponGrantItem>),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn grants<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Query(pagination): Query<Pagination>,
) -> Result<List<CouponGrantItem>, SumidaError> {
  Ok(CouponService::new(state, session).grants(id, pagination).await?.into())
}

/// The coupons issued to the caller and not yet spent.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = MINE,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_mine",
  description = HOLDER,
  params(Pagination),
  responses(
    (status = 200, description = "One page of the caller's coupons, and how many there are behind it", body = List<MyCouponItem>),
    Unreadable,
    Refused,
  ),
))]
async fn mine<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(pagination): Query<Pagination>,
) -> Result<List<MyCouponItem>, SumidaError> {
  Ok(CouponService::new(state, session).my_coupons(pagination).await?.into())
}

/// Spend a coupon that gives something outright.
///
/// This is for the rewards that stand on their own — credits, or a free period of a plan — and
/// `reward` says which of the two it turned out to be. A discount coupon is not redeemed here; it
/// is named at checkout, where there is a price for it to come off.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = REDEEM,
  context_path = MOUNT,
  tag = COUPON_TAG,
  operation_id = "coupon_redeem",
  description = HOLDER,
  request_body = RedeemCoupon,
  responses(
    (status = 200, description = "What the coupon turned into", body = RedeemResult),
    (
      status = 400,
      description = "The code is unknown, expired, out of redemptions, not issued to this caller, \
                     or carries a discount rather than an outright reward.",
      body = ErrorBody,
    ),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn redeem<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<RedeemCoupon>,
) -> Result<RedeemResult, SumidaError> {
  CouponService::new(state, session).redeem(body).await
}

impl<S: SumidaState> ApiController<S> for CouponController<S> {
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "coupon")
      .read(
        Router::new()
          .route("/", routing::get(list::<S>))
          .route(GRANTS, routing::get(grants::<S>)),
      )
      .descriptor::<CouponFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      // 배포와 회수는 쿠폰 자체를 손보는 일이라 `update` 로 묶는다. 서비스 쪽 행 점검도 같은
      // `coupon#edit` 이므로 게이트를 통과한 호출자가 행에서 다른 이유로 막히지 않는다.
      .update(
        Router::new()
          .route(ONE, routing::patch(update::<S>))
          .route(GRANT, routing::post(grant::<S>))
          .route(GRANT, routing::delete(revoke::<S>))
          .route(GRANT_BULK, routing::post(grant_bulk::<S>)),
      )
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      // 받은 것을 쓰는 길은 관리 게이트 바깥이다. 자기 앞으로 발행된 쿠폰을 보고 쓰는 데에
      // 쿠폰 관리 권한을 요구하면, 평범한 계정이 자기가 받은 것을 쓰지 못한다.
      .ungated(
        Router::new()
          .route(MINE, routing::get(mine::<S>))
          .route(REDEEM, routing::post(redeem::<S>)),
      )
      .build()
  }
}
