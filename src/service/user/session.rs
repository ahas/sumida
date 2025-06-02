use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use chrono::Utc;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tower_sessions::{Expiry, Session};
use uaparser::{Parser, UserAgentParser};

use crate::core::db;
use crate::core::error::SumidaError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserProfile {
  pub username: String,
  pub full_name: String,
  pub organization: OrganizationData,
  pub role: RoleData,
  pub ability: JsonValue,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub first_seen_at: DateTimeWithTimeZone,
  pub last_seen_at: DateTimeWithTimeZone,
  pub expiry: Option<Expiry>,
}

impl Default for UserProfile {
  fn default() -> Self {
    Self {
      username: "".to_owned(),
      full_name: "".to_owned(),
      organization: OrganizationData::default(),
      role: RoleData::default(),
      ability: json!({}),
      is_admin: false,
      is_authorized: false,
      first_seen_at: Utc::now().into(),
      last_seen_at: Utc::now().into(),
      expiry: None,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrganizationData {
  pub id: i32,
  pub name: String,
}

impl Default for OrganizationData {
  fn default() -> Self {
    Self {
      id: 0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoleData {
  pub id: i32,
  pub name: String,
}

impl Default for RoleData {
  fn default() -> Self {
    Self {
      id: 0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserEnvironment {
  pub ip_address: String,
  pub user_agent: String,

  pub os_family: String,
  pub os_major: Option<String>,
  pub os_minor: Option<String>,
  pub os_patch: Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand: Option<String>,
  pub device_model: Option<String>,
}

impl UserEnvironment {
  pub fn from_request_parts(req: &Parts) -> Result<Self, anyhow::Error> {
    let user_agent = value
      .headers
      .get(axum::http::header::USER_AGENT)
      .and_then(|ua| ua.to_str().ok())
      .map(|s| s.to_string())
      .unwrap_or_default();
    let ua_regexes_bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ua-regexes.yml"));
    let ua_parser = UserAgentParser::builder()
      .build_from_bytes(ua_regexes_bytes)
      .map_err(|_| SumidaError::InvalidUserAgent)?;
    let ua_device = ua_parser.parse_device(user_agent.as_str());
    let ua_os = ua_parser.parse_os(user_agent.as_str());

    Self {
      ip_address: value.extensions.get::<ConnectInfo<SocketAddr>>().unwrap().ip().to_string(),
      user_agent,
      os_family: ua_os.family.to_string(),
      os_major: ua_os.major.map(|x| x.to_string()),
      os_minor: ua_os.minor.map(|x| x.to_string()),
      os_patch: ua_os.patch.map(|x| x.to_string()),
      os_patch_minor: ua_os.patch_minor.map(|x| x.to_string()),
      device_family: ua_device.family.to_string(),
      device_brand: ua_device.brand.map(|x| x.to_string()),
      device_model: ua_device.model.map(|x| x.to_string()),
    }
  }
}

#[derive(Clone, Debug)]
pub struct UserSession {
  pub session: Session,
  pub profile: UserProfile,
  pub env: UserEnvironment,
}

impl UserSession {
  pub const USER_PROFILE_KEY: &'static str = "user.profile";
  pub const USER_ENV_KEY: &'static str = "user.env";

  pub async fn update(
    session: &Session,
    profile: &UserProfile,
    env: &UserEnvironment,
  ) -> Result<(), anyhow::Error> {
    session.insert(Self::USER_PROFILE_KEY, profile.clone()).await?;
    session.insert(Self::USER_ENV_KEY, env.clone()).await?;
    session.set_expiry(profile.expiry);
    session.save().await?;

    Ok(())
  }

  pub async fn verify(&self) -> Result<(), SumidaError> {
    let mut profile = self.profile.clone();

    if !profile.is_authorized {
      return Err(if profile.username.is_empty() {
        SumidaError::NoCredentials
      } else {
        SumidaError::NotAuthorized(profile.username)
      });
    }

    let db_conn = db::get_connection();
    let user = crate::entity::users::Entity::find_by_id(&profile.username)
      .one(db_conn)
      .await?
      .ok_or(SumidaError::InvalidAccount)?;
    let organization = crate::entity::organizations::Entity::find_by_id(user.organization)
      .one(db_conn)
      .await?
      .ok_or(SumidaError::NoUserOrganization(user.organization))?;
    let role = crate::entity::roles::Entity::find_by_id(user.role)
      .one(db_conn)
      .await?
      .ok_or(SumidaError::NoUserRole(user.role))?;

    profile.username = user.username.clone();
    profile.organization = OrganizationData {
      id: organization.id,
      name: organization.name.clone(),
    };
    profile.role = RoleData {
      id: role.id,
      name: role.name.clone(),
    };
    profile.is_admin = user.is_admin;
    profile.last_seen_at = Utc::now().into();
    profile.ability = role.ability;

    Self::update(&self.session, &profile, &self.env).await?;

    Ok(())
  }

  pub async fn sign_in(
    &self,
    user: &crate::entity::users::Model,
    expiry: Option<Expiry>,
  ) -> Result<(), SumidaError> {
    let db_conn = db::get_connection();
    let organization =
      user.get_organization(db_conn).await?.ok_or(SumidaError::NoUserOrganization(user.organization))?;
    let organization = OrganizationData {
      id: organization.id,
      name: organization.name.clone(),
    };
    let role = user.get_role(db_conn).await?.ok_or(SumidaError::NoUserRole(user.role))?;
    let role = RoleData {
      id: role.id,
      name: role.name.clone(),
    };

    Self::update(
      &self.session,
      &UserProfile {
        is_authorized: true,
        username: user.username.clone(),
        full_name: user.full_name.clone().unwrap_or("".to_owned()),
        organization,
        role,
        is_admin: user.is_admin,
        expiry,
        ..Default::default()
      },
      &self.env,
    )
    .await?;

    Ok(())
  }
}

impl<S> FromRequestParts<S> for UserSession
where
  S: Send + Sync,
{
  type Rejection = SumidaError;

  async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let session = Session::from_request_parts(req, state).await?;

    let mut profile: UserProfile = session.get(Self::USER_PROFILE_KEY).await.unwrap().unwrap_or_default();
    profile.last_seen_at = Utc::now().into();

    let user_agent = req
      .headers
      .get(axum::http::header::USER_AGENT)
      .and_then(|ua| ua.to_str().ok())
      .map(|s| s.to_string())
      .unwrap_or_default();

    let env = UserEnvironment::from(req);

    Self::update(&session, &profile, &env).await?;

    Ok(Self { session })
  }
}
