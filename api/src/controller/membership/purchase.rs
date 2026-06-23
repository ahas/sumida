use std::marker::PhantomData;

use axum::extract::{Query, State};
use axum::{Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
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
    Query(pagination): Query<Pagination>,
  ) -> Result<List<PurchaseListItem>, SumidaError> {
    Ok(PurchaseService::new(state, session).list(filter, pagination).await?.into())
  }
}

impl<S: SumidaState> ApiController<S> for PurchaseController<S> {
  fn router(self, state: &S) -> Router<S> {
    Router::new().route("/", routing::get(Self::list)).route_layer(user_guard!(state))
  }
}
