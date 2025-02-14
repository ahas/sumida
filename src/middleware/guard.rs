use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use chrono::Utc;
#[cfg(feature = "jwt")]
use jsonwebtoken as jwt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
#[cfg(feature = "jwt")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "aws")]
use crate::core::aws;
use crate::core::db;
use crate::core::error::SumidaError;
#[cfg(feature = "jwt")]
use crate::core::settings;
use crate::entity::prelude::*;
use crate::service::prelude::*;
use crate::service::user::session::{OrganizationData, RoleData};

#[macro_export]
macro_rules! ability_guard {
  ($topic: ident, $action: ident) => {
    axum::middleware::from_fn(
      |session: $crate::service::user::session::UserSession,
       req: axum::extract::Request,
       next: axum::middleware::Next| async move {
        let topic = stringify!($topic);
        let action = stringify!($action);

        let Some(ability) = session.user_data.ability.get(topic) else {
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

#[cfg(feature = "jwt")]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwtClaims {
  pub user: String,
  pub exp: usize,
}

#[cfg(feature = "jwt")]
async fn verify_jwt(session: &UserSession, token: &str) -> Result<(), SumidaError> {
  let settings = settings::get();
  let secret;

  #[cfg(feature = "jwt-secret-store-aws-ssm")]
  {
    let ssm_client = aws_sdk_ssm::Client::new(aws::get_sdk_config());
    secret = ssm_client
      .get_parameter()
      .name(format!("/joy/{}/jwt-secret", settings.server.stage))
      .with_decryption(true)
      .send()
      .await
      .unwrap()
      .parameter
      .and_then(|param| param.value)
      .ok_or(SumidaError::NoJwtSecret)?;
  }
  #[cfg(feature = "jwt-secret-store-static")]
  {
    secret = settings.jwt.as_ref().unwrap().secret.clone().unwrap_or_default();
  }

  let token = jwt::decode::<JwtClaims>(
    token,
    &jwt::DecodingKey::from_secret(secret.as_ref()),
    &jwt::Validation::default(),
  )
  .map_err(|_err| SumidaError::InvalidJwt(token.to_string()))?;

  let db_conn = db::get_connection();
  let user = Users::find_by_id(&token.claims.user)
    .one(db_conn)
    .await?
    .ok_or_else(|| SumidaError::NoUserAccount(token.claims.user.to_string()))?;
  let organization = Organizations::find_by_id(user.organization)
    .one(db_conn)
    .await?
    .ok_or_else(|| SumidaError::NoUserOrganization(user.organization))?;
  let role =
    Roles::find_by_id(user.role).one(db_conn).await?.ok_or_else(|| SumidaError::NoUserRole(user.role))?;

  let mut user_data = session.user_data.clone();
  user_data.username = token.claims.user.clone();
  user_data.organization = OrganizationData {
    id: organization.id,
    name: organization.name.clone(),
  };
  user_data.role = RoleData {
    id: role.id,
    name: role.name.clone(),
  };
  user_data.is_admin = false;
  user_data.last_seen_at = Utc::now().into();
  user_data.ability = role.ability;

  UserSession::update(&session.session, &user_data).await?;

  Ok(())
}

async fn verify_api_key(session: &UserSession, token: &str) -> Result<(), SumidaError> {
  let db_conn = db::get_connection();
  let api_key = ApiKeys::find()
    .filter(crate::entity::api_keys::Column::Key.eq(token))
    .one(db_conn)
    .await?
    .ok_or_else(|| SumidaError::InvalidApiKey(token.to_string()))?;
  let mut user_data = session.user_data.clone();
  let user = if let Some(username) = api_key.user {
    Users::find_by_id(&username).one(db_conn).await?
  } else {
    None
  };

  if let Some(user) = user {
    let organization = Organizations::find_by_id(user.organization)
      .one(db_conn)
      .await?
      .ok_or_else(|| SumidaError::NoUserOrganization(user.organization))?;
    let role =
      Roles::find_by_id(user.role).one(db_conn).await?.ok_or_else(|| SumidaError::NoUserRole(user.role))?;

    user_data.username = user.username.clone();
    user_data.organization = OrganizationData {
      id: organization.id,
      name: organization.name.clone(),
    };
    user_data.role = RoleData {
      id: role.id,
      name: role.name.clone(),
    };
    user_data.is_admin = user.is_admin;
    user_data.ability = api_key.ability.unwrap_or(role.ability);
  } else {
    user_data.username = "".to_string();
    user_data.organization = OrganizationData::default();
    user_data.role = RoleData::default();
    user_data.is_admin = false;
    user_data.ability = api_key.ability.unwrap_or_default();
  }

  user_data.last_seen_at = Utc::now().into();

  UserSession::update(&session.session, &user_data).await?;

  Ok(())
}

pub async fn verify(session: &UserSession, headers: &HeaderMap<HeaderValue>) -> Result<bool, SumidaError> {
  if let Some(v) = headers.get::<String>("Authorization".into()) {
    let pair = v.to_str().unwrap_or_default().split_whitespace().map(str::trim).collect::<Vec<&str>>();

    if pair.len() < 2 {
      return Err(SumidaError::InvalidAuthorizationToken(v.to_str().unwrap_or("").to_string()));
    }

    let method = pair[0];
    let token = pair[1];

    match method.to_uppercase().as_str() {
      #[cfg(feature = "jwt")]
      "BEARER" => verify_jwt(session, token).await?,
      "APIKEY" => verify_api_key(session, token).await?,
      _ => return Err(SumidaError::InvalidAuthorizationMethod(method.to_string())),
    }
  } else {
    session.verify().await?
  }

  Ok(true)
}

#[allow(unused)]
pub async fn user_guard_fn(
  session: UserSession,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  verify(&session, req.headers()).await?;

  Ok(next.run(req).await)
}

#[allow(unused)]
pub async fn admin_guard_fn(
  session: UserSession,
  req: Request,
  next: Next,
) -> Result<Response, SumidaError> {
  match session.user_data.is_admin {
    true => Ok(next.run(req).await),
    false => Err(SumidaError::OnlyAdminAllowed),
  }
}
