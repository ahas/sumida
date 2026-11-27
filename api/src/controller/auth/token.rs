use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde::{Deserialize, Serialize};

use crate::SumidaState;
use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::AUTH_TAG as TAG;
#[cfg(feature = "openapi")]
use crate::openapi::{ErrorBody, Unreadable, WrongBody};
use crate::service::auth::helper::TokenPair;
use crate::service::auth::token::{AuthTokenService, create_jwt_store};
use crate::service::prelude::*;

/// Where this controller's routes stand. The router nests here and the document hangs its paths
/// here, so the two cannot come to name different places.
pub const MOUNT: &str = "/auth/token";

/// The rotation route, named once for the router and the document both.
const REFRESH: &str = "/refresh";

/// The credentials a client posts here, as the reader reads them.
#[cfg(feature = "openapi")]
pub fn sign_in_example() -> serde_json::Value {
  serde_json::json!({ "username": "someone", "password": "the password", "rememberMe": false })
}

/// A refresh token as the reader reads it. **The key is `refresh_token`**, not `refreshToken`:
/// these two bodies are read without the renaming the rest of this API applies.
#[cfg(feature = "openapi")]
pub fn refresh_example() -> serde_json::Value {
  serde_json::json!({ "refresh_token": "<the refresh_token from a previous response>" })
}

/// The same, for the revocation route, where it may also be left out.
#[cfg(feature = "openapi")]
pub fn revoke_example() -> serde_json::Value {
  serde_json::json!({ "refresh_token": "<the refresh_token to revoke, or omit this key>" })
}

#[derive(Clone, Debug, Default)]
pub struct AuthTokenController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

/// The body the rotation route reads.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RefreshTokenRequest {
  /// The refresh token handed out with the access token it is being traded for.
  pub refresh_token: String,
}

/// The body the revocation route reads.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LogoutRequest {
  /// The refresh token to revoke. Leaving it out revokes only the access token the request
  /// carries, and the refresh token then keeps working until it expires.
  pub refresh_token: Option<String>,
}

/// What the revocation route answers with.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoggedOut {
  /// A fixed line. Nothing branches on it.
  pub message: String,
}

impl<S: SumidaState> AuthTokenController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// Signs in with a username and password and issues a token pair.
///
/// The other way in. It takes the same credentials as `POST /auth/session` and answers with tokens
/// instead of a cookie: the caller then sends `Authorization: Bearer <accessToken>` on every later
/// request, and trades the refresh token for a new pair when the access token runs out.
///
/// **The keys in the response are `access_token`, `refresh_token`, `token_type` and `expires_in`**
/// — this body is written without the renaming applied elsewhere on this API.
///
/// `rememberMe` lengthens the life of the refresh token only; the access token's life is a
/// deployment setting and is reported as `expires_in` seconds.
///
/// Consecutive password failures on one account can get the account or the address banned, which
/// then refuses even the right password.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "issue_tokens",
  request_body(content = LoginSession, example = sign_in_example),
  security(()),
  responses(
    (
      status = 200,
      description = "The pair. `access_token` authenticates a request; `refresh_token` buys the \
                     next pair; `token_type` is `Bearer`; `expires_in` is how many seconds the \
                     access token has left.",
      body = TokenPair,
    ),
    (
      status = 401,
      description = "The credentials are refused. No account answers to that name, the password \
                     does not match, the account is disabled, or a ban catches this attempt. A \
                     deployment that has no JWT secret to sign with answers here as well, with a \
                     `code` of its own.",
      body = ErrorBody,
    ),
    Unreadable,
    WrongBody,
  ),
))]
async fn login<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<LoginSession>,
) -> Result<(StatusCode, Json<TokenPair>), SumidaError> {
  let jwt_store = create_jwt_store().await?;
  let mut service = AuthTokenService::new(state, session, jwt_store);
  let token_pair = service.login(body).await?;
  Ok((StatusCode::OK, Json(token_pair)))
}

/// Trades a refresh token for a new pair.
///
/// The refresh token is the credential here, so no other one is needed and none is read: whoever
/// holds it gets the pair. The presented token is **rotated** — it is deleted as the new one is
/// stored, so it buys exactly one pair and a replay of it is refused.
///
/// The account is re-examined at this moment, not merely at sign-in: a token whose account has
/// since been disabled, banned, or has reset its credentials buys nothing.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = REFRESH,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "rotate_tokens",
  request_body(content = RefreshTokenRequest, example = refresh_example),
  security(()),
  responses(
    (status = 200, description = "A new pair. The presented refresh token no longer works.", body = TokenPair),
    (
      status = 401,
      description = "The refresh token is refused — expired, malformed, already rotated away, \
                     revoked, an access token presented in its place, or belonging to an account \
                     that may no longer be let in.",
      body = ErrorBody,
    ),
    Unreadable,
    WrongBody,
  ),
))]
async fn refresh<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<RefreshTokenRequest>,
) -> Result<(StatusCode, Json<TokenPair>), SumidaError> {
  let jwt_store = create_jwt_store().await?;
  let mut service = AuthTokenService::new(state, session, jwt_store);
  let token_pair = service.refresh_token(body.refresh_token).await?;
  Ok((StatusCode::OK, Json(token_pair)))
}

/// Revokes tokens.
///
/// Two tokens can be revoked here and each is optional, which is why this route asks for no
/// credential of its own: the access token is read from `Authorization: Bearer <token>` if it is
/// there, and the refresh token from the body if it is there. Both are blacklisted until they would
/// have expired anyway, so revocation takes effect on the next request rather than at the next
/// expiry. The session this request arrives on is destroyed as well.
///
/// The body is optional: leaving it out, or sending `{}`, revokes only the access token. Presenting
/// neither token is not an error, and revokes nothing.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "revoke_tokens",
  request_body(content = Option<LogoutRequest>, example = revoke_example),
  security((), ("bearer" = [])),
  responses(
    (status = 200, description = "Whatever was presented is revoked.", body = LoggedOut),
    Unreadable,
    WrongBody,
  ),
))]
async fn logout<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  headers: axum::http::HeaderMap,
  body: Option<Json<LogoutRequest>>,
) -> Result<(StatusCode, Json<LoggedOut>), SumidaError> {
  // A request without a body names no refresh token. A body with the wrong media type is still refused.
  let refresh_token = body.and_then(|Json(body)| body.refresh_token);

  // Extract the current access token from Authorization: Bearer <access_token> and revoke it as well.
  let access_token =
    headers
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
  service.logout(refresh_token, access_token).await?;
  Ok((
    StatusCode::OK,
    Json(LoggedOut {
      message: "Logged out successfully".to_owned(),
    }),
  ))
}

impl<S: SumidaState> ApiController<S> for AuthTokenController<S>
where
  S: Clone + Send + Sync + 'static,
{
  /// No guard stands over any of these. Two of the routes take the credentials themselves, and the
  /// third revokes whatever it was handed — asking for a credential to give one up would leave a
  /// caller whose access token has expired with no way to revoke the refresh token behind it.
  fn router(self, _state: &S) -> axum::Router<S> {
    Router::<S>::new()
      .route("/", routing::post(login::<S>))
      .route(REFRESH, routing::post(refresh::<S>))
      .route("/", routing::delete(logout::<S>))
  }
}
