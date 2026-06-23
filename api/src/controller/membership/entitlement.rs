use std::marker::PhantomData;

use axum::extract::State;
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

#[derive(Clone, Debug, Default)]
pub struct EntitlementController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> EntitlementController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  /// Check (and, when credits are used, consume for) an action. This is the
  /// hybrid subscription-or-credit gate, exposed so applications/tests can drive it.
  async fn check(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<RequireActionRequest>,
  ) -> Result<EntitlementOutcome, SumidaError> {
    EntitlementService::new(state, session)
      .require_action(&body.action, body.owner_kind, body.owner_id, body.idempotency_key.as_deref())
      .await
  }
}

impl<S: SumidaState> ApiController<S> for EntitlementController<S> {
  fn router(self, state: &S) -> Router<S> {
    Router::new()
      .route("/check", routing::post(Self::check))
      .route_layer(user_guard!(state))
  }
}
