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

  /// `POST /auth/token` — exchange `LoginSession` credentials (username/password) for a
  /// `TokenPair { access_token, refresh_token }`.
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

  /// `POST /auth/token/refresh` — body `{ "refreshToken": "..." }`. Returns a freshly rotated
  /// `TokenPair`; the presented refresh token is invalidated.
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

  /// `DELETE /auth/token` — body `{ "refreshToken"?: "..." }` plus the `Authorization: Bearer
  /// <access_token>` header. Blacklists both the refresh token (if given) and the current access
  /// token for immediate invalidation.
  async fn logout(
    session: UserSession,
    State(state): State<S>,
    headers: axum::http::HeaderMap,
    Json(body): Json<LogoutRequest>,
  ) -> Result<(StatusCode, Json<JsonValue>), SumidaError> {
    // Extract the current access token from Authorization: Bearer <access_token> and revoke it as well.
    let access_token = headers
      .get(axum::http::header::AUTHORIZATION)
      .and_then(|v| v.to_str().ok())
      .and_then(|v| {
        let mut parts = v.split_whitespace();
        match (parts.next(), parts.next()) {
          (Some(scheme), Some(token)) if scheme.eq_ignore_ascii_case("bearer") => Some(token.to_string()),
          _ => None,
        }
      });

    let jwt_store = create_jwt_store().await?;
    let mut service = AuthTokenService::new(state, session, jwt_store);
    service.logout(body.refresh_token, access_token).await?;
    Ok((StatusCode::OK, Json(serde_json::json!({ "message": "Logged out successfully" }))))
  }
}

impl<S: SumidaState> ApiController<S> for AuthTokenController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, _state: &S) -> axum::Router<S> {
    // POST / = login, POST /refresh = rotate, DELETE / = logout. Nested under `/auth/token`.
    Router::<S>::new()
      .route("/", routing::post(Self::login))
      .route("/refresh", routing::post(Self::refresh))
      .route("/", routing::delete(Self::logout))
  }
}
