use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

#[derive(Clone, Debug, Default)]
pub struct CouponController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> CouponController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<CouponFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<CouponItem>, SumidaError> {
    Ok(CouponService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateCoupon>,
  ) -> Result<CouponCreated, SumidaError> {
    CouponService::new(state, session).create(body).await
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateCoupon>,
  ) -> Result<CouponUpdated, SumidaError> {
    CouponService::new(state, session).update(id, body).await
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<CouponDeleted, SumidaError> {
    CouponService::new(state, session).delete(id).await
  }

  async fn grant(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<GrantCoupon>,
  ) -> Result<CouponGranted, SumidaError> {
    CouponService::new(state, session).grant_to(id, body).await
  }

  async fn grant_bulk(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<BulkGrantCoupon>,
  ) -> Result<BulkGrantResult, SumidaError> {
    CouponService::new(state, session).grant_bulk(id, body).await
  }

  async fn revoke(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Query(owner): Query<GrantCoupon>,
  ) -> Result<CouponRevoked, SumidaError> {
    CouponService::new(state, session).revoke(id, owner).await
  }

  async fn grants(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<CouponGrantItem>, SumidaError> {
    Ok(CouponService::new(state, session).grants(id, pagination).await?.into())
  }

  async fn mine(
    session: UserSession,
    State(state): State<S>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<MyCouponItem>, SumidaError> {
    Ok(CouponService::new(state, session).my_coupons(pagination).await?.into())
  }

  async fn redeem(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<RedeemCoupon>,
  ) -> Result<RedeemResult, SumidaError> {
    CouponService::new(state, session).redeem(body).await
  }
}

impl<S: SumidaState> ApiController<S> for CouponController<S> {
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "coupon")
      .read(
        Router::new()
          .route("/", routing::get(Self::list))
          .route("/{id}/grants", routing::get(Self::grants)),
      )
      .descriptor::<CouponFilter>()
      .create(Router::new().route("/", routing::post(Self::create)))
      // 배포와 회수는 쿠폰 자체를 손보는 일이라 `update` 로 묶는다. 서비스 쪽 행 점검도 같은
      // `coupon#edit` 이므로 게이트를 통과한 호출자가 행에서 다른 이유로 막히지 않는다.
      .update(
        Router::new()
          .route("/{id}", routing::patch(Self::update))
          .route("/{id}/grant", routing::post(Self::grant).delete(Self::revoke))
          .route("/{id}/grant-bulk", routing::post(Self::grant_bulk)),
      )
      .delete(Router::new().route("/{id}", routing::delete(Self::delete)))
      // 받은 것을 쓰는 길은 관리 게이트 바깥이다. 자기 앞으로 발행된 쿠폰을 보고 쓰는 데에
      // 쿠폰 관리 권한을 요구하면, 평범한 계정이 자기가 받은 것을 쓰지 못한다.
      .ungated(
        Router::new()
          .route("/mine", routing::get(Self::mine))
          .route("/redeem", routing::post(Self::redeem)),
      )
      .build()
  }
}
