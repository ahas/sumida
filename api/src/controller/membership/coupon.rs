use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_route};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

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
    // Admin-only management (the service also enforces is_admin).
    //
    // 서술은 목록과 같은 게이트를 받아야 하고, 이 목록이 서 있는 것은 아래 `user_guard!` 하나뿐
    // 이므로 게이트를 스스로 달지 않는 라우트를 그 안에 합친다.
    Router::new()
      .merge(filter_descriptor_route::<S, CouponFilter>())
      .route("/", routing::get(Self::list).post(Self::create))
      .route("/mine", routing::get(Self::mine))
      .route("/redeem", routing::post(Self::redeem))
      .route("/{id}", routing::patch(Self::update).delete(Self::delete))
      .route("/{id}/grant", routing::post(Self::grant).delete(Self::revoke))
      .route("/{id}/grant-bulk", routing::post(Self::grant_bulk))
      .route("/{id}/grants", routing::get(Self::grants))
      .route_layer(user_guard!(state))
  }
}
