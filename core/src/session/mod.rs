#[cfg(feature = "session-store-aws-dynamodb")]
pub mod dynamodb_store;
#[cfg(feature = "session-store-redis")]
pub mod redis_store;

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use axum_client_ip::ClientIp;
use chrono::{Duration, Utc};
use entity::prelude::*;
use entity::user_sign_in_logs::{SignInCode, SignInMethod};
use error::SumidaError;
use lazy_static::lazy_static;
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::sea_query::ExprTrait;
use sea_orm::{Condition, InsertResult, TransactionTrait, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tower_sessions::{Expiry, Session};
use uaparser::{Parser, UserAgentParser};

use crate::{db, settings};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
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
  pub async fn update<C: ConnectionTrait>(
    &mut self,
    user: entity::users::Model,
    db: &C,
  ) -> anyhow::Result<()> {
    let organization = entity::organizations::Entity::find_by_id(user.organization)
      .one(db)
      .await?
      .ok_or(SumidaError::NoUserOrganization(user.organization))?;
    let role = entity::roles::Entity::find_by_id(user.role)
      .one(db)
      .await?
      .ok_or(SumidaError::NoUserRole(user.role))?;

    self.username = user.username.clone();
    self.full_name = user.full_name.unwrap_or_default().clone();
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

impl IntoResponse for UserProfile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
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
#[serde(rename_all = "camelCase")]
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

lazy_static! {
  static ref UA_PARSER: UserAgentParser = {
    UserAgentParser::builder()
      .build_from_bytes(include_bytes!("ua-regexes.yml"))
      .expect("valid ua-regexes.yml")
  };
}

impl<S> FromRequestParts<S> for UserEnvironment
where
  S: Send + Sync,
{
  type Rejection = SumidaError;

  async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let user_agent = req
      .headers
      .get(axum::http::header::USER_AGENT)
      .and_then(|ua| ua.to_str().ok())
      .map(|s| s.to_string())
      .unwrap_or_default();
    // let ua_regexes_bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ua-regexes.yml"));
    let ua_parser = &*UA_PARSER;
    let ua_device = ua_parser.parse_device(user_agent.as_str());
    let ua_os = ua_parser.parse_os(user_agent.as_str());
    let ClientIp(ip) = ClientIp::from_request_parts(req, state).await.map_err(anyhow::Error::from)?;

    Ok(Self {
      ip_address: ip.to_string(),
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserState {
  pub failed_sign_in_attempts: u32,
}

#[derive(Clone, Debug)]
pub struct UserContext {
  pub session: Session,
  pub profile: UserProfile,
  pub env: UserEnvironment,
  pub state: UserState,
}

impl UserContext {
  pub const USER_PROFILE_KEY: &'static str = "user.profile";
  pub const USER_ENV_KEY: &'static str = "user.env";
  pub const USER_STATE_KEY: &'static str = "user.state";

  pub async fn sign_in(&mut self, username: &str, expiry: Option<Expiry>) -> Result<(), SumidaError> {
    self.profile.username = username.to_owned();
    self.profile.is_authorized = true;
    self.profile.expiry = expiry;
    self.profile.first_seen_at = Utc::now().fixed_offset();
    self.state.failed_sign_in_attempts = 0;

    self.identify().await?;

    Ok(())
  }

  pub async fn write_log<C: ConnectionTrait>(
    &self,
    username: Option<&str>,
    method: SignInMethod,
    code: SignInCode,
    db: &C,
  ) -> Result<InsertResult<entity::user_sign_in_logs::ActiveModel>, DbErr> {
    UserSignInLogs::insert(entity::user_sign_in_logs::ActiveModel {
      user: Set(username.map(String::from).unwrap_or_else(|| self.profile.username.clone())),
      method: Set(method),
      code: Set(code),
      ip_address: Set(self.env.ip_address.clone()),
      user_agent: Set(self.env.user_agent.clone()),
      device_family: Set(self.env.device_family.clone()),
      device_brand: Set(self.env.device_brand.clone()),
      device_model: Set(self.env.device_model.clone()),
      os_family: Set(self.env.os_family.clone()),
      os_major: Set(self.env.os_major.clone()),
      os_minor: Set(self.env.os_minor.clone()),
      os_patch: Set(self.env.os_patch.clone()),
      os_patch_minor: Set(self.env.os_patch_minor.clone()),
      ..Default::default()
    })
    .exec(db)
    .await
  }

  pub fn resolve_username(&self, explicit: Option<&str>) -> String {
    explicit.map(String::from).unwrap_or_else(|| self.profile.username.clone())
  }

  pub async fn block_if_exceeded_limit<C: ConnectionTrait>(
    &mut self,
    username: Option<&str>,
    db: &C,
  ) -> Result<bool, SumidaError> {
    let settings = &settings::get().security.block;
    let username = self.resolve_username(username);

    self.state.failed_sign_in_attempts += 1;
    self.session.insert(Self::USER_STATE_KEY, self.state.clone()).await.map_err(anyhow::Error::from)?;

    if self.state.failed_sign_in_attempts > settings.max_failed_attempts {
      let duration = humantime::parse_duration(&settings.failure_reset_after)
        .ok()
        .and_then(|x| Duration::from_std(x).ok());
      let reason = Some("system blocked");

      match settings.target.as_str() {
        "ip" => self.block_ip(duration, "", reason, db).await?,
        "account" => self.block_account(Some(username.as_str()), duration, "", reason, db).await?,
        _ => {}
      };

      return Ok(true);
    }

    Ok(false)
  }

  pub async fn block_ip<C: ConnectionTrait>(
    &self,
    duration: Option<Duration>,
    reason: &str,
    note: Option<&str>,
    db: &C,
  ) -> Result<(), SumidaError> {
    let expires_at = duration.map(|x| (Utc::now() + x).fixed_offset());

    UserBlocks::insert(entity::user_blocks::ActiveModel {
      ip_address: Set(Some(self.env.ip_address.clone())),
      blocked_at: Set(Utc::now().fixed_offset()),
      reason: Set(reason.to_string()),
      expires_at: Set(expires_at),
      note: Set(note.map(String::from)),
      ..Default::default()
    })
    .exec(db)
    .await?;

    Ok(())
  }

  pub async fn block_account<C: ConnectionTrait>(
    &self,
    username: Option<&str>,
    duration: Option<Duration>,
    reason: &str,
    note: Option<&str>,
    db: &C,
  ) -> Result<(), SumidaError> {
    let username = self.resolve_username(username);
    let expires_at = duration.map(|x| (Utc::now() + x).fixed_offset());

    UserBlocks::insert(entity::user_blocks::ActiveModel {
      user: Set(Some(username)),
      blocked_at: Set(Utc::now().fixed_offset()),
      reason: Set(reason.to_string()),
      expires_at: Set(expires_at),
      note: Set(note.map(String::from)),
      ..Default::default()
    })
    .exec(db)
    .await?;

    Ok(())
  }

  pub async fn check_blocked<C: ConnectionTrait>(
    &self,
    username: Option<&str>,
    db: &C,
  ) -> Result<bool, SumidaError> {
    let now = Utc::now();
    let mut condition = Condition::any();

    if let Some(username) = username {
      condition = condition.add(entity::user_blocks::Column::User.eq(username));
    }

    condition = condition.add(entity::user_blocks::Column::IpAddress.eq(&self.env.ip_address));
    condition = Condition::all().add(condition);
    condition = condition.add(entity::user_blocks::Column::LiftedAt.is_null());
    condition = condition.add(
      Condition::any()
        .add(entity::user_blocks::Column::ExpiresAt.is_null())
        .add(entity::user_blocks::Column::ExpiresAt.lt(now)),
    );

    Ok(UserBlocks::find().filter(condition).count(db).await? > 0)
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

      Users::update(entity::users::ActiveModel {
        username: Unchanged(profile.username.clone()),
        last_sign_in_at: Set(Some(Utc::now().fixed_offset())),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      let user = entity::users::Entity::find_by_id(&profile.username)
        .one(&txn)
        .await?
        .ok_or(SumidaError::InvalidAccount)?;
      profile.update(user, &txn).await?;
      txn.commit().await?;
    } else {
      self.profile = UserProfile::default();
    }

    self.profile.last_seen_at = Utc::now().into();
    self.store().await?;

    Ok(())
  }

  pub async fn store(&self) -> Result<(), anyhow::Error> {
    self.session.insert(Self::USER_PROFILE_KEY, self.profile.clone()).await?;
    self.session.insert(Self::USER_ENV_KEY, self.env.clone()).await?;
    self.session.insert(Self::USER_STATE_KEY, self.state.clone()).await?;
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
    let user_state = session.get(Self::USER_STATE_KEY).await.unwrap().unwrap_or_default();
    let env = UserEnvironment::from_request_parts(req, state).await?;

    let mut user_ctx = Self {
      session,
      profile,
      env,
      state: user_state,
    };
    user_ctx.profile.last_seen_at = Utc::now().into();
    user_ctx
      .session
      .insert(Self::USER_PROFILE_KEY, user_ctx.profile.clone())
      .await
      .map_err(anyhow::Error::from)?;
    user_ctx.session.save().await.map_err(anyhow::Error::from)?;

    Ok(user_ctx)
  }
}
