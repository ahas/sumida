use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::FromRequestParts;
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
use tower_sessions::Expiry;

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

      let mut session = match UserSession::from_request_parts(&mut parts, &()).await {
        Ok(session) => session,
        Err(_) => {
          let error_response = SumidaError::NoCredentials.into_response();
          return Ok(error_response);
        }
      };

      let req = Request::from_parts(parts, body);

      if let Err(err) = verify(&mut session, state, req.headers()).await {
        let error_response = err.into_response();
        return Ok(error_response);
      }

      inner.call(req).await
    })
  }
}

pub async fn verify_jwt<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  token: &str,
) -> Result<(), SumidaError> {
  let settings = infra::settings::get();
  // Allow only access tokens. Includes token_type validation.
  let claims = helper::verify_access_token(token).await?;
  let expiry = settings.jwt.as_ref().ok_or(SumidaError::NoJwtSecret)?.expiry.access_token_expiry()?;

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
  let Some(user) = Users::find_by_id(user_id).one(db).await? else {
    return Err(SumidaError::InvalidAccount);
  };
  if !user.is_enabled {
    return Err(SumidaError::InactiveAccount);
  }
  if session.check_banned(Some(user_id), db).await? {
    return Err(SumidaError::AccountBanned);
  }

  session.authorize(user_id, &claims.username, Some(expiry), db).await?;

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
  if api_key.expires_at.is_some_and(|expires_at| expires_at < Utc::now().fixed_offset()) {
    return Err(SumidaError::InvalidApiKey);
  }

  let user = Users::find_by_id(api_key.user_id).one(db).await?;

  if let Some(user) = user {
    // Reject inactive/banned accounts (same policy as the session login path).
    if !user.is_enabled {
      return Err(SumidaError::InactiveAccount);
    }
    if session.check_banned(Some(user.id), db).await? {
      return Err(SumidaError::AccountBanned);
    }

    session.authorize(user.id, &user.username, Some(Expiry::OnSessionEnd), db).await?;

    // Key scope policy (intended design):
    //   - is_secured=true : sets profile.api_key → who() evaluates only the `api:KEY` identifier,
    //                       isolating permissions to the access_policy scope explicitly granted to the key.
    //   - is_secured=false: does not set api_key → who() expands all of the owner's User/Role/Org/Admin
    //                       identifiers, so the key inherits the owner's full permissions as-is.
    // In short: an unsecured key = owner full access (for convenience), a secured key = scope-limited. Announce this distinction to downstream.
    // Policies are stored under the hash identifier (`api:<hash>`), so put the hash into profile.api_key too.
    if api_key.is_secured {
      session.profile.api_key = Some(key_hash);
    }
  } else {
    return Err(SumidaError::InvalidAccount);
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
      return Err(SumidaError::InvalidAuthorizationToken);
    }

    let method = pair[0];
    let token = pair[1];

    match method.to_uppercase().as_str() {
      "BEARER" => verify_jwt(session, state, token).await?,
      "APIKEY" => verify_api_key(session, state, token).await?,
      _ => return Err(SumidaError::InvalidAuthorizationMethod(method.to_string())),
    }
  } else {
    session.verify()?;
  }

  Ok(true)
}

#[macro_export]
macro_rules! user_guard {
  ($state:expr) => {
    $crate::layer::UserGuardLayer::new($state.clone())
  };
}
