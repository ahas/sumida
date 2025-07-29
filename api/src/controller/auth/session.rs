use infra::session::UserContext;

use axum::{Json, Router, extract::State, http::StatusCode, routing};
use error::SumidaError;

use crate::{controller::ApiController, service::prelude::*, user_guard};

#[derive(Clone)]
pub struct AuthSessionController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> AuthSessionController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn login(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<LoginSession>,
  ) -> Result<StatusCode, SumidaError> {
    AuthSessionService::new(user_ctx).login(body).await?;
    Ok(StatusCode::OK)
  }

  async fn logout(user_ctx: UserContext, State(_state): State<S>) -> Result<StatusCode, SumidaError> {
    AuthSessionService::new(user_ctx).logout().await?;
    Ok(StatusCode::OK)
  }
}

impl<S> ApiController<S> for AuthSessionController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::<S>::new() //
      .route("/", routing::delete(Self::logout))
      .route_layer(user_guard!(state))
      .route("/", routing::post(Self::login))
  }
}
