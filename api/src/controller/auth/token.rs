use axum::http::StatusCode;
use axum::{Json, Router, extract::State, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::SumidaState;
use crate::controller::ApiController;
use crate::service::auth::helper::TokenPair;
use crate::service::auth::token::{AuthTokenService, create_jwt_store};
use crate::service::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct AuthTokenController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

#[derive(Debug, Deserialize)]
struct RefreshTokenRequest {
  refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct LogoutRequest {
  refresh_token: Option<String>,
}

impl<S: SumidaState> AuthTokenController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn login(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<LoginSession>,
  ) -> Result<(StatusCode, Json<TokenPair>), SumidaError> {
    let jwt_store = create_jwt_store().await?;
    let mut service = AuthTokenService::new(state, session, jwt_store);
    let token_pair = service.login(body).await?;
    Ok((StatusCode::OK, Json(token_pair)))
  }

  async fn refresh(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<RefreshTokenRequest>,
  ) -> Result<(StatusCode, Json<TokenPair>), SumidaError> {
    let jwt_store = create_jwt_store().await?;
    let mut service = AuthTokenService::new(state, session, jwt_store);
    let token_pair = service.refresh_token(body.refresh_token).await?;
    Ok((StatusCode::OK, Json(token_pair)))
  }

  async fn logout(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<LogoutRequest>,
  ) -> Result<(StatusCode, Json<JsonValue>), SumidaError> {
    let jwt_store = create_jwt_store().await?;
    let mut service = AuthTokenService::new(state, session, jwt_store);
    service.logout(body.refresh_token).await?;
    Ok((StatusCode::OK, Json(serde_json::json!({ "message": "Logged out successfully" }))))
  }
}

impl<S: SumidaState> ApiController<S> for AuthTokenController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, _state: &S) -> axum::Router<S> {
    Router::<S>::new()
      .route("/", routing::post(Self::login))
      .route("/refresh", routing::post(Self::refresh))
      .route("/", routing::delete(Self::logout))
  }
}
