use infra::session::UserSession;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing};
use error::SumidaError;
use serde_json::json;

use crate::{controller::ApiController, layer::UserGuardLayer, service::prelude::*, SumidaState};

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
