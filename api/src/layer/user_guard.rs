use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::{HeaderMap, HeaderValue, Request, Response};
use axum::response::IntoResponse;
use entity::prelude::*;
use error::SumidaError;
use futures::future::BoxFuture;
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
  // access token만 허용. token_type 검증 포함.
  let claims = helper::verify_access_token(token).await?;
  let expiry = settings.jwt.as_ref().ok_or(SumidaError::NoJwtSecret)?.expiry.access_token_expiry()?;

  // 표준 클레임 sub(문자열)을 user_id(i32)로 파싱.
  let user_id: i32 = claims.sub.parse().map_err(|_| SumidaError::InvalidJwt)?;

  session.authorize(user_id, &claims.username, Some(expiry), state.db()).await?;

  Ok(())
}

pub async fn verify_api_key<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  key: &str,
) -> Result<(), SumidaError> {
  let db = state.db();
  let api_key = ApiKeys::find()
    .filter(entity::api_keys::Column::Key.eq(key))
    .one(db)
    .await?
    .ok_or(SumidaError::InvalidApiKey)?;
  let user = Users::find_by_id(api_key.user_id).one(db).await?;

  if let Some(user) = user {
    session.authorize(user.id, &user.username, Some(Expiry::OnSessionEnd), db).await?;

    if api_key.is_secured {
      session.profile.api_key = Some(key.to_owned());
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
