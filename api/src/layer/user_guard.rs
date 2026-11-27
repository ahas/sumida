use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Request, Response};
use axum::response::IntoResponse;
use chrono::Utc;
use entity::prelude::*;
use error::SumidaError;
use futures::future::BoxFuture;
use infra::jwt::JwtTokenStore;
use infra::session::UserSession;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tower::{Layer, Service};

use crate::SumidaState;
use crate::service::auth::helper;

#[derive(Clone)]
pub struct UserGuardLayer<S> {
  state: S,
}

impl<S> UserGuardLayer<S> {
  pub fn new(state: S) -> Self {
    Self { state }
  }
}

impl<S, T> Layer<T> for UserGuardLayer<S>
where
  S: Clone,
{
  type Service = UserGuardService<S, T>;

  fn layer(&self, inner: T) -> Self::Service {
    UserGuardService {
      state: self.state.clone(),
      inner,
    }
  }
}

#[derive(Clone)]
pub struct UserGuardService<S, T> {
  state: S,
  inner: T,
}

impl<S, T> Service<Request<Body>> for UserGuardService<S, T>
where
  S: SumidaState,
  T: Service<Request<Body>, Response = Response<Body>, Error = std::convert::Infallible>
    + Clone
    + Send
    + 'static,
  T::Future: Send + 'static,
{
  type Response = Response<Body>;
  type Error = std::convert::Infallible;
  type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    self.inner.poll_ready(cx)
  }

  fn call(&mut self, req: Request<Body>) -> Self::Future {
    let state = self.state.clone();
    let mut inner = self.inner.clone();

    Box::pin(async move {
      let (mut parts, body) = req.into_parts();

      if let Err(err) = admit(&mut parts, state).await {
        return Ok(err.into_response());
      }

      inner.call(Request::from_parts(parts, body)).await
    })
  }
}

/// Establishes who a request is, or the reason it is refused.
///
/// A header credential is checked and established for this request alone: its identity is handed to
/// every later extractor through the request's extensions, and nothing of it reaches the cookie
/// session. A request with no header credential is admitted on its cookie session, re-checked
/// against the account as it stands now — and, when it would change something, only from a page
/// this server answers (see [`refuse_cross_site`]).
pub async fn admit<S: SumidaState>(parts: &mut Parts, state: S) -> Result<UserSession, SumidaError> {
  refuse_cross_site(parts)?;

  let mut session = UserSession::from_request_parts(parts, &())
    .await
    .map_err(|_| SumidaError::NoCredentials)?;

  verify(&mut session, state, &parts.headers).await?;

  if session.is_request_local().await {
    parts.extensions.insert(session.principal());
  }

  Ok(session)
}

/// Refuses a state-changing request that rides on the cookie from another site's page.
///
/// A browser attaches the session cookie to whatever request a page sends to this server, whichever
/// site the page is on, as far as the cookie's `SameSite` lets it — and `None` lets it everywhere.
/// So a request that would change something (any method but `GET`, `HEAD`, `OPTIONS` and `TRACE`)
/// and authenticates by the cookie is refused when the page it names, in `Origin` or else
/// `Referer`, is neither the server's own nor one of `server.cors.allow_origins`. A request that
/// carries a header credential is unaffected: another site's page cannot add one to a visitor's
/// request, so its presence already says the request was not forged.
fn refuse_cross_site(parts: &Parts) -> Result<(), SumidaError> {
  if parts.method.is_safe() || parts.headers.contains_key(AUTHORIZATION) {
    return Ok(());
  }
  if infra::origin::source_allowed(parts, infra::origin::allowed_origins()) {
    return Ok(());
  }
  Err(SumidaError::CrossSiteRequest)
}

pub async fn verify_jwt<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  token: &str,
) -> Result<(), SumidaError> {
  // Allow only access tokens. Includes token_type validation.
  let claims = helper::verify_access_token(token).await?;

  // Parse the standard claim sub (a string) into user_id (i32).
  let user_id: i32 = claims.sub.parse().map_err(|_| SumidaError::InvalidJwt)?;

  // Block logged-out / forcibly revoked access tokens (jti blacklist lookup).
  // This turns the access token from stateless into stateful (a store lookup on every auth), but
  // enables immediate invalidation. The blacklist is kept with a TTL only until the token's expiry.
  if crate::service::auth::token::shared_jwt_store()
    .await?
    .is_token_blacklisted(&claims.jti)
    .await?
  {
    return Err(SumidaError::InvalidJwt);
  }

  // Reject inactive/banned accounts even with a valid access token (same policy as the session login path).
  let db = state.db();
  let user = session.admitted_account(user_id, db).await?;

  // A token the account has since disowned. `iat` is the moment it was issued, which is what the
  // reset is compared against.
  if infra::session::credentials_reset_since(&user, helper::issued_at(&claims)) {
    return Err(SumidaError::InvalidJwt);
  }

  // The token is presented, and checked, again on every request; it authorizes this one alone.
  session.authorize_request(user, None, db).await?;

  Ok(())
}

pub async fn verify_api_key<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  key: &str,
) -> Result<(), SumidaError> {
  let db = state.db();
  // The DB stores only the hash, so hash the incoming plaintext key to look it up.
  let key_hash = crate::service::api_key::hash_api_key(key);
  let api_key = ApiKeys::find()
    .filter(entity::api_keys::Column::Key.eq(&key_hash))
    .one(db)
    .await?
    .ok_or(SumidaError::InvalidApiKey)?;

  // Reject expired keys.
  if api_key.expires_at.is_some_and(|expires_at| expires_at < Utc::now()) {
    return Err(SumidaError::InvalidApiKey);
  }

  // Reject inactive/banned accounts (same policy as the session login path). A key is a credential
  // of its own, issued and revoked on its own terms, so a password reset does not reach it.
  let user = session.admitted_account(api_key.user_id, db).await?;

  // Key scope policy:
  //   - is_secured=true : profile.api_key carries the key's hash, so `SubjectSet::of` judges the
  //                       request as `apikey:<hash>` alone — the grants given to the key and nothing
  //                       of its owner's user, role, organization or administrator standing.
  //   - is_secured=false: profile.api_key stays unset, so the request is judged as the owner.
  // Grants are stored under the hash identifier, so the hash is what goes into the profile. It is
  // part of the identity the request is established with, never added to it afterwards: a reader
  // that ran in between would see the owner.
  let secured = api_key.is_secured.then_some(key_hash);
  session.authorize_request(user, secured, db).await?;

  Ok(())
}

/// Reject a session cookie whose account may no longer be let in.
///
/// The two paths above re-read the account on every request; a cookie has to be held to the same
/// rule. What it carries was written at sign-in, and a ban or a deactivation raised after that
/// moment is exactly the case where the two disagree — the cookie still says authorized, and the
/// account no longer is.
pub async fn verify_session<S: SumidaState>(session: &UserSession, state: S) -> Result<(), SumidaError> {
  session.verify()?;

  let db = state.db();
  let user = session.admitted_account(session.profile.id, db).await?;

  // A session the account has since disowned. It was established when it was authorized.
  if infra::session::credentials_reset_since(&user, session.profile.first_seen_at) {
    return Err(SumidaError::NoCredentials);
  }

  Ok(())
}

pub async fn verify<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  headers: &HeaderMap<HeaderValue>,
) -> Result<bool, SumidaError> {
  if let Some(v) = headers.get::<String>("Authorization".into()) {
    let pair = v.to_str().unwrap_or_default().split_whitespace().map(str::trim).collect::<Vec<&str>>();

    if pair.len() < 2 {
      return Err(uniform_credential_refusal(SumidaError::InvalidAuthorizationToken));
    }

    let method = pair[0];
    let token = pair[1];

    let verified = match method.to_uppercase().as_str() {
      "BEARER" => verify_jwt(session, state, token).await,
      "APIKEY" => verify_api_key(session, state, token).await,
      _ => Err(SumidaError::InvalidAuthorizationMethod(method.to_string())),
    };
    verified.map_err(uniform_credential_refusal)?;
  } else {
    verify_session(session, state).await?;
  }

  Ok(true)
}

/// One answer for every credential that does not stand.
///
/// A malformed header, an unknown scheme, a key or token that does not verify, and a deployment
/// with no token secret each had their own code, so an anonymous caller could map which schemes a
/// deployment takes and how it is configured. They all answer as a missing credential does. An
/// account that a valid credential names but that may not come in still says why: only the holder
/// of that credential sees it.
fn uniform_credential_refusal(error: SumidaError) -> SumidaError {
  match error {
    SumidaError::InvalidAuthorizationToken
    | SumidaError::InvalidAuthorizationMethod(_)
    | SumidaError::InvalidApiKey
    | SumidaError::InvalidJwt
    | SumidaError::NoJwtSecret => {
      tracing::debug!("credential refused: {error}");
      SumidaError::NoCredentials
    }
    other => other,
  }
}

#[macro_export]
macro_rules! user_guard {
  ($state:expr) => {
    $crate::layer::UserGuardLayer::new($state.clone())
  };
}
