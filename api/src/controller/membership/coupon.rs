use std::marker::PhantomData;

use axum::extract::{Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
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
    Query(pagination): Query<Pagination>,
  ) -> Result<List<CouponItem>, SumidaError> {
    Ok(CouponService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateCoupon>,
  ) -> Result<CouponCreated, SumidaError> {
    CouponService::new(state, session).create(body).await
  }
}

impl<S: SumidaState> ApiController<S> for CouponController<S> {
  fn router(self, state: &S) -> Router<S> {
    // Admin-only management (the service also enforces is_admin).
    Router::new()
      .route("/", routing::get(Self::list).post(Self::create))
      .route_layer(user_guard!(state))
  }
}
