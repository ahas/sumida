use axum::{
  body::Body,
  extract::FromRequestParts,
  http::{HeaderMap, HeaderValue, Request, Response},
  response::IntoResponse,
};
use cfg_if::cfg_if;
use entity::prelude::*;
use error::SumidaError;
use futures::future::BoxFuture;
use infra::session::UserSession;
use jsonwebtoken as jwt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use serde::{Deserialize, Serialize};
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tower_sessions::Expiry;

use crate::SumidaState;

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
  T: Service<Request<Body>, Response = Response<Body>, Error = std::convert::Infallible> + Clone + Send + 'static,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwtClaims {
  pub user_id: i32,
  pub username: String,
  pub exp: usize,
}

pub async fn verify_jwt<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  token: &str,
) -> Result<(), SumidaError> {
  let settings = infra::settings::get();
  #[cfg(not(any(feature = "jwt-secret-store-static", feature = "jwt-secret-store-aws-ssm")))]
  compile_error!("Either `jwt-secret-store-static` or `jwt-secret-store-aws-ssm` feature must be enabled");

  #[allow(unused_assignments)]
  let mut secret = "".to_string();

  cfg_if! {
      if #[cfg(feature = "jwt-secret-store-static")] {
          secret = settings
              .jwt
              .as_ref()
              .and_then(|x| x.secret.clone())
              .unwrap_or_default();
      } else if #[cfg(feature = "jwt-secret-store-aws-ssm")] {
          use infra::aws;
          let ssm_client = aws_sdk_ssm::Client::new(aws::get_sdk_config());
          secret = ssm_client
              .get_parameter()
              .name(format!("/trainer/{}/jwt-secret", settings.server.stage))
              .with_decryption(true)
              .send()
              .await
              .map_err(|e| SumidaError::Unsupported(e.to_string()))?
              .parameter
              .and_then(|param| param.value)
              .ok_or(SumidaError::NoJwtSecret)?;
      }
  };

  let token = jwt::decode::<JwtClaims>(
    token,
    &jwt::DecodingKey::from_secret(secret.as_ref()),
    &jwt::Validation::default(),
  )
  .map_err(|_err| SumidaError::InvalidJwt)?;
  let expiry = settings.jwt.as_ref().ok_or(SumidaError::NoJwtSecret)?.expiry.access_token_expiry()?;

  session.authorize(token.claims.user_id, &token.claims.username, Some(expiry), state.db()).await?;

  Ok(())
}

pub async fn verify_api_key<S: SumidaState>(
  session: &mut UserSession,
  state: S,
  key: &str,
) -> Result<(), SumidaError> {
  let txn = state.db().begin().await?;

  let api_key = ApiKeys::find()
    .filter(entity::api_keys::Column::Key.eq(key))
    .one(&txn)
    .await?
    .ok_or(SumidaError::InvalidApiKey)?;
  let user = Users::find_by_id(api_key.user_id).one(&txn).await?;

  if let Some(user) = user {
    session.authorize(user.id, &user.username, Some(Expiry::OnSessionEnd), &txn).await?;

    if api_key.is_secured {
      session.profile.api_key = Some(key.to_owned());
    }
  } else {
    return Err(SumidaError::InvalidAccount);
  }

  txn.commit().await?;

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
