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
#[cfg(feature = "openapi")]
use crate::openapi::AUTH_TAG as TAG;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, Unauthenticated, Unreadable, WrongBody};
use crate::service::prelude::*;

/// Where this controller's routes stand. The router nests here and the document hangs its paths
/// here, so the two cannot come to name different places.
pub const MOUNT: &str = "/auth/session";

/// The credentials a client posts here, as the reader reads them.
///
/// The document offers this as its example and a check feeds the same value to the reader that
/// will receive it, so what a client copies out of the document is something that has been read.
#[cfg(feature = "openapi")]
pub fn sign_in_example() -> serde_json::Value {
  json!({ "username": "someone", "password": "the password", "rememberMe": false })
}

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
}

/// Signs in with a username and password and starts a session.
///
/// This is the way in that needs no credential of its own, and everything else on this surface
/// begins here. On success the response carries the session cookie in `Set-Cookie`; the caller
/// sends that cookie back on every later request, and nothing else is handed out — there is no
/// token in this response. The session id is reissued at this moment, so a cookie held from before
/// the sign-in is not the one that is now good.
///
/// `rememberMe` decides how long the session lives, and leaving it out is not the same as sending
/// `false`: omitted, the cookie lasts only as long as the browser session; `false` gives it the
/// deployment's ordinary idle window; `true` gives it the longer remembered window. Both windows
/// are deployment settings.
///
/// Consecutive password failures on one account can get the account or the address banned, which
/// then refuses even the right password.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "sign_in",
  request_body(content = LoginSession, example = sign_in_example),
  security(()),
  responses(
    (
      status = 200,
      description = "Signed in. The session cookie is in `Set-Cookie` and the body carries nothing.",
      body = EmptyObject,
    ),
    (
      status = 401,
      description = "The credentials are refused. No account answers to that name, the password \
                     does not match, the account is disabled, or a ban catches this attempt — the \
                     status does not tell them apart, and `code` does.",
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
) -> Result<impl IntoResponse, SumidaError> {
  AuthSessionService::new(state, session).login(body).await?;
  Ok((StatusCode::OK, Json(json!({}))))
}

/// Ends the session this request arrives on.
///
/// The stored session is destroyed rather than emptied, so the id that arrived stops naming
/// anything. A request that arrives on a bearer token or an api key is admitted here too, and what
/// it ends is the session that was stood up for it — the token itself keeps working until
/// `DELETE /auth/token` revokes it.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "sign_out",
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (
      status = 200,
      description = "The session is gone. The body carries nothing.",
      body = EmptyObject,
    ),
    Unauthenticated,
  ),
))]
async fn logout<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
) -> Result<impl IntoResponse, SumidaError> {
  AuthSessionService::new(state, session).logout().await?;
  Ok((StatusCode::OK, Json(json!({}))))
}

impl<S: SumidaState> ApiController<S> for AuthSessionController<S>
where
  S: Clone + Send + Sync + 'static,
{
  /// The guard covers the sign-out and stops short of the sign-in.
  ///
  /// A layer reaches the routes registered before it and no others, so the order of these three
  /// statements is what makes signing in possible: a caller that has no session yet cannot be
  /// asked for one.
  fn router(self, state: &S) -> axum::Router<S> {
    Router::<S>::new() //
      .route("/", routing::delete(logout::<S>))
      .layer(UserGuardLayer::new(state.clone()))
      .route("/", routing::post(login::<S>))
  }
}
