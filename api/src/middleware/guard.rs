use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use cfg_if::cfg_if;
use entity::prelude::*;
use error::SumidaError;
use infra::db;
use infra::session::UserSession;
use infra::settings;
use jsonwebtoken as jwt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use serde::{Deserialize, Serialize};
use tower_sessions::Expiry;

use crate::SumidaState;

#[macro_export]
macro_rules! service_ap {
  ($ns: literal, $action: ident) => {
    axum::middleware::from_fn(|session, req, next| {
      let ns = $ns.to_string();
      let action = stringify!($action).to_string();

      $crate::middleware::guard::service_ap_fn(session, ns, action, req, next)
    })
  };
}

#[macro_export]
macro_rules! user_guard {
  ($state: ident) => {
    axum::middleware::from_fn_with_state($state.clone(), $crate::middleware::guard::user_guard_fn)
  };
}

#[macro_export]
macro_rules! admin_guard {
  ($state: ident) => {
    axum::middleware::from_fn_with_state($state.clone(), $crate::middleware::guard::admin_guard_fn)
  };
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
  let settings = settings::get();
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

#[allow(dead_code)]
pub async fn service_ap_fn<S: SumidaState>(
  session: UserSession,
  state: S,
  ns: String,
  action: String,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  use crate::util::ap::{self, ApGuard};

  let is_allowed = ap::ns(ns) //
    .object("service")
    .action(action)
    .who(&session)
    .can_access(db::get_connection())
    .await;

  if is_allowed {
    Ok(next.run(req).await)
  } else {
    Err(SumidaError::NoServiceAvailable)
  }
}

#[allow(dead_code)]
pub async fn user_guard_fn(
  mut session: UserSession,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  verify(&mut session, req.headers()).await?;

  Ok(next.run(req).await)
}

#[allow(dead_code)]
pub async fn admin_guard_fn<S: SumidaState>(
  session: UserSession,
  state: S,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  match session.profile.is_admin {
    true => Ok(next.run(req).await),
    false => Err(SumidaError::OnlyAdminAllowed),
  }
}
