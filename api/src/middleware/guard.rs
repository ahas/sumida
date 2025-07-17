use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use cfg_if::cfg_if;
use core::db;
use core::session::UserContext;
use core::settings;
use entity::prelude::*;
use error::SumidaError;
use jsonwebtoken as jwt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use tower_sessions::Expiry;

#[macro_export]
macro_rules! ability_guard {
  ($topic: ident, $action: ident) => {
    axum::middleware::from_fn(
      |user_ctx: $crate::sumida_core::session::UserContext,
       req: axum::extract::Request,
       next: axum::middleware::Next| async move {
        let topic = stringify!($topic);
        let action = stringify!($action);

        let Some(ability) = user_ctx.profile.ability.get(topic) else {
          return Err(SumidaError::NoAbilityTopic(topic.to_string()));
        };
        let Some(allowed_prop) = ability.get(action) else {
          return Err(SumidaError::NoAbilityAction(topic.to_string(), action.to_string()));
        };
        let allowed = allowed_prop.as_bool().unwrap_or(false);

        match allowed {
          true => Ok(next.run(req).await),
          false => Err(SumidaError::NoAbilityAction(topic.to_string(), action.to_string())),
        }
      },
    )
  };
}

#[macro_export]
macro_rules! service_ap {
  ($ns: ident, $action: ident) => {
    axum::middleware::from_fn(
      |user_ctx: $crate::sumida_core::session::UserContext,
       req: axum::extract::Request,
       next: axum::middleware::Next| async move {
        use crate::util::ap::Ap;
        use core::db;

        let ns = stringify!($ns);
        let action = stringify!($action);

        let is_allowed = Ap::new(ns)
          .object(sea_orm::sea_query::Expr::val("service").into())
          .action(action)
          .owner(&user_ctx)
          .check_allowed(db::get_connection())
          .await;

        if is_allowed {
          Ok(next.run(req).await)
        } else {
          Err(SumidaError::NoServiceAvailable)
        }
      },
    )
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

async fn verify_jwt(user_ctx: &mut UserContext, token: &str) -> Result<(), SumidaError> {
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
      use core::aws;
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

  user_ctx.authorize(token.claims.user_id, &token.claims.username, Some(expiry)).await?;

  Ok(())
}

async fn verify_api_key(user_ctx: &mut UserContext, token: &str) -> Result<(), SumidaError> {
  let db_conn = db::get_connection();
  let api_key = ApiKeys::find()
    .filter(entity::api_keys::Column::Key.eq(token))
    .one(db_conn)
    .await?
    .ok_or(SumidaError::InvalidApiKey)?;
  let user = Users::find_by_id(api_key.user_id).one(db_conn).await?;

  if let Some(user) = user {
    user_ctx.authorize(user.id, &user.username, Some(Expiry::OnSessionEnd)).await?;
  } else {
    // UserSignInLogs::write(
    //   user_data.username,
    //   SignInMethod::ApiKey,
    //   SignInCode::AccountNotFound,
    //   user_env,
    //   db,
    // );
    return Err(SumidaError::InvalidAccount);
  }

  Ok(())
}

pub async fn verify(
  user_ctx: &mut UserContext,
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
      "BEARER" => verify_jwt(user_ctx, token).await?,
      "APIKEY" => verify_api_key(user_ctx, token).await?,
      _ => return Err(SumidaError::InvalidAuthorizationMethod(method.to_string())),
    }
  } else {
    user_ctx.verify()?;
  }

  Ok(true)
}

#[allow(dead_code)]
pub async fn user_guard_fn(
  mut user_ctx: UserContext,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  verify(&mut user_ctx, req.headers()).await?;

  Ok(next.run(req).await)
}

#[allow(dead_code)]
pub async fn admin_guard_fn(
  user_ctx: UserContext,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  match user_ctx.profile.is_admin {
    true => Ok(next.run(req).await),
    false => Err(SumidaError::OnlyAdminAllowed),
  }
}
