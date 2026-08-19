use std::marker::PhantomData;

use axum::extract::{Query, State};
use axum::{Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_route};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

#[derive(Clone, Debug, Default)]
pub struct PurchaseController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> PurchaseController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<PurchaseFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<PurchaseListItem>, SumidaError> {
    Ok(PurchaseService::new(state, session).list(filter, conditions, pagination).await?.into())
  }
}

impl<S: SumidaState> ApiController<S> for PurchaseController<S> {
  fn router(self, state: &S) -> Router<S> {
    // 서술은 목록과 같은 게이트를 받아야 하고, 이 목록이 서 있는 것은 아래 `user_guard!` 하나뿐
    // 이므로 게이트를 스스로 달지 않는 라우트를 그 안에 합친다.
    Router::new()
      .route("/", routing::get(Self::list))
      .merge(filter_descriptor_route::<S, PurchaseFilter>())
      .route_layer(user_guard!(state))
  }
}
