use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde_json::json;

use crate::SumidaState;
use crate::controller::ApiController;
use crate::layer::UserGuardLayer;
use crate::service::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct AuthSessionController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> AuthSessionController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn login(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<LoginSession>,
  ) -> Result<impl IntoResponse, SumidaError> {
    AuthSessionService::new(state, session).login(body).await?;
    Ok((StatusCode::OK, Json(json!({}))))
  }

  async fn logout(session: UserSession, State(state): State<S>) -> Result<impl IntoResponse, SumidaError> {
    AuthSessionService::new(state, session).logout().await?;
    Ok((StatusCode::OK, Json(json!({}))))
  }
}

impl<S: SumidaState> ApiController<S> for AuthSessionController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::<S>::new() //
      .route("/", routing::delete(Self::logout))
      .layer(UserGuardLayer::new(state.clone()))
      .route("/", routing::post(Self::login))
  }
}
