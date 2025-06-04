use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use chrono::Utc;
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::{prelude::*, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tower_sessions::{Expiry, Session};
use uaparser::{Parser, UserAgentParser};

use crate::core::db;
use crate::core::error::SumidaError;
use crate::entity::prelude::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserProfile {
  pub username: String,
  pub full_name: String,
  pub organization: UserOrganization,
  pub role: UserRole,
  pub ability: JsonValue,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub first_seen_at: DateTimeWithTimeZone,
  pub last_seen_at: DateTimeWithTimeZone,
  pub expiry: Option<Expiry>,
}

impl UserProfile {
  pub async fn update<'db, C: ConnectionTrait>(
    &mut self,
    user: crate::entity::users::Model,
    db: &'db C,
  ) -> anyhow::Result<()> {
    let organization = crate::entity::organizations::Entity::find_by_id(user.organization)
      .one(db)
      .await?
      .ok_or(SumidaError::NoUserOrganization(user.organization))?;
    let role = crate::entity::roles::Entity::find_by_id(user.role)
      .one(db)
      .await?
      .ok_or(SumidaError::NoUserRole(user.role))?;

    self.username = user.username.clone();
    self.organization = UserOrganization {
      id: organization.id,
      name: organization.name.clone(),
    };
    self.role = UserRole {
      id: role.id,
      name: role.name.clone(),
    };
    self.is_admin = user.is_admin;
    self.ability = role.ability;

    Ok(())
  }
}

impl Default for UserProfile {
  fn default() -> Self {
    Self {
      username: "".to_owned(),
      full_name: "".to_owned(),
      organization: UserOrganization::default(),
      role: UserRole::default(),
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
pub struct UserOrganization {
  pub id: i32,
  pub name: String,
}

impl Default for UserOrganization {
  fn default() -> Self {
    Self {
      id: 0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRole {
  pub id: i32,
  pub name: String,
}

impl Default for UserRole {
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
impl<S> FromRequestParts<S> for UserEnvironment
where
  S: Send + Sync,
{
  type Rejection = SumidaError;

  async fn from_request_parts(req: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
    let user_agent = req
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

    Ok(Self {
      ip_address: req.extensions.get::<ConnectInfo<SocketAddr>>().unwrap().ip().to_string(),
      user_agent: user_agent.clone(),
      os_family: ua_os.family.to_string(),
      os_major: ua_os.major.map(|x| x.to_string()),
      os_minor: ua_os.minor.map(|x| x.to_string()),
      os_patch: ua_os.patch.map(|x| x.to_string()),
      os_patch_minor: ua_os.patch_minor.map(|x| x.to_string()),
      device_family: ua_device.family.to_string(),
      device_brand: ua_device.brand.map(|x| x.to_string()),
      device_model: ua_device.model.map(|x| x.to_string()),
    })
  }
}

#[derive(Clone, Debug)]
pub struct UserContext {
  pub session: Session,
  pub profile: UserProfile,
  pub env: UserEnvironment,
}

impl UserContext {
  pub const USER_PROFILE_KEY: &'static str = "user.profile";
  pub const USER_ENV_KEY: &'static str = "user.env";

  pub async fn sign_in(&mut self, username: &str, expiry: Option<Expiry>) -> Result<(), SumidaError> {
    self.profile.username = username.to_owned();
    self.profile.is_authorized = true;
    self.profile.expiry = expiry;

    self.identify().await?;

    Ok(())
  }

  pub fn verify(&self) -> Result<(), SumidaError> {
    if self.profile.is_authorized {
      Ok(())
    } else {
      Err(if self.profile.username.is_empty() {
        SumidaError::NoCredentials
      } else {
        SumidaError::NotAuthorized(self.profile.username.clone())
      })
    }
  }

  pub async fn identify(&mut self) -> Result<(), SumidaError> {
    if self.profile.is_authorized {
      let profile = &mut self.profile;
      let db_conn = db::get_connection();
      let txn = db_conn.begin().await?;

      Users::update(crate::entity::users::ActiveModel {
        username: Unchanged(profile.username.clone()),
        last_sign_in_at: Set(Some(Utc::now().fixed_offset())),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      let user = crate::entity::users::Entity::find_by_id(&profile.username)
        .one(&txn)
        .await?
        .ok_or(SumidaError::InvalidAccount)?;
      profile.update(user, &txn).await?;
      txn.commit().await?;
    } else {
      self.profile = UserProfile::default();
    }

    self.profile.last_seen_at = Utc::now().into();

    Ok(())
  }

  pub async fn store(&self) -> Result<(), anyhow::Error> {
    self.session.insert(Self::USER_PROFILE_KEY, self.profile.clone()).await?;
    self.session.insert(Self::USER_ENV_KEY, self.env.clone()).await?;
    self.session.set_expiry(self.profile.expiry);
    self.session.save().await?;

    Ok(())
  }
}

impl<S> FromRequestParts<S> for UserContext
where
  S: Send + Sync,
{
  type Rejection = SumidaError;

  async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let session = Session::from_request_parts(req, state).await?;
    let profile = session.get(Self::USER_PROFILE_KEY).await.unwrap().unwrap_or_default();
    let env = UserEnvironment::from_request_parts(req, state).await?;

    let mut user_ctx = Self {
      session,
      profile,
      env,
    };
    user_ctx.identify().await?;
    user_ctx.store().await?;

    Ok(user_ctx)
  }
}
