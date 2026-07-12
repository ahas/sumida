#[cfg(feature = "session-store-aws-dynamodb")]
pub mod dynamodb_store;
#[cfg(feature = "session-store-redis")]
pub mod redis_store;

/// Build the session store selected by the active `session-store-*` feature.
///
/// This self-branches on the compile-time feature (same idiom as `infra::jwt`), so the caller
/// (an app bin) just does `let store = session::new_store(&settings.session).await?;` and passes
/// it to `api::layer::session::new(store)` — no store-picking `#[cfg]` in the app.
/// Only the enabled backend's deps are linked, so there is no runtime-branch binary bloat.
///
/// The concrete return type differs per feature (`RedisStore` vs `DynamoDBStore`); both implement
/// `SessionStore`, so it is returned as `impl SessionStore`.
#[cfg(feature = "session-store-redis")]
pub async fn new_store(
  settings: &crate::settings::session::SessionSettings,
) -> Result<impl tower_sessions::SessionStore + Clone, anyhow::Error> {
  let redis = settings
    .redis
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("missing `session.redis` in settings"))?;
  Ok(redis_store::RedisSessionStore::new(redis).await?.store())
}

#[cfg(all(feature = "session-store-aws-dynamodb", not(feature = "session-store-redis")))]
pub async fn new_store(
  settings: &crate::settings::session::SessionSettings,
) -> Result<impl tower_sessions::SessionStore + Clone, anyhow::Error> {
  let dynamodb = settings
    .dynamodb
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("missing `session.dynamodb` in settings"))?;
  dynamodb_store::new(dynamodb).await
}

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use axum_client_ip::ClientIp;
use chrono::{Duration, Utc};
use entity::access_logs::{AccessMethod, AccessResultCode};
use entity::prelude::*;
use error::SumidaError;
use lazy_static::lazy_static;
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::prelude::*;
use sea_orm::{Condition, FromQueryResult, InsertResult, QueryOrder, QuerySelect, TransactionTrait};
use serde::{Deserialize, Serialize};
use tower_sessions::{Expiry, Session};
use uaparser::{Parser, UserAgentParser};

use crate::settings;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
  pub id: i32,
  pub username: String,
  pub full_name: String,
  pub organization: UserOrganization,
  pub role: UserRole,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub api_key: Option<String>,
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
    let organization = entity::organizations::Entity::find_by_id(user.organization_id)
      .one(db)
      .await?
      .ok_or(SumidaError::NoUserOrganization(user.organization_id))?;
    let role = entity::roles::Entity::find_by_id(user.role_id)
      .one(db)
      .await?
      .ok_or(SumidaError::NoUserRole(user.role_id))?;

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

    Ok(())
  }

  pub fn get_id(&self) -> Option<i32> {
    if self.is_authorized { Some(self.id) } else { None }
  }
}

impl Default for UserProfile {
  fn default() -> Self {
    Self {
      id: 0,
      username: "".to_owned(),
      full_name: "".to_owned(),
      organization: UserOrganization::default(),
      role: UserRole::default(),
      is_admin: false,
      is_authorized: false,
      api_key: None,
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
pub struct UserState {}

#[derive(Clone, Debug)]
pub struct UserSession {
  pub inner: Session,
  pub profile: UserProfile,
  pub env: UserEnvironment,
  pub state: UserState,
}

impl UserSession {
  pub const USER_PROFILE_KEY: &'static str = "user.profile";
  pub const USER_ENV_KEY: &'static str = "user.env";
  pub const USER_STATE_KEY: &'static str = "user.state";

  pub async fn authorize<C: ConnectionTrait + TransactionTrait>(
    &mut self,
    id: i32,
    username: &str,
    expiry: Option<Expiry>,
    db: &C,
  ) -> Result<(), SumidaError> {
    self.profile.id = id;
    self.profile.username = username.to_owned();
    self.profile.is_authorized = true;
    self.profile.expiry = expiry;
    self.profile.first_seen_at = Utc::now().fixed_offset();

    self.inner.set_expiry(expiry);
    self.identify(db).await?;

    // Session fixation defense: reissue the session ID on login.
    // (Prevents a pre-auth anonymous session ID from continuing to be used after authentication.)
    self.inner.cycle_id().await.map_err(anyhow::Error::from)?;

    Ok(())
  }

  pub async fn write_log(
    &self,
    user_id: Option<i32>,
    method: AccessMethod,
    code: AccessResultCode,
    db: &DatabaseConnection,
  ) -> Result<InsertResult<entity::access_logs::ActiveModel>, DbErr> {
    let count: Option<i32> = if matches!(code, AccessResultCode::PasswordMismatch) {
      #[derive(FromQueryResult)]
      struct LastAccessLog {
        count: Option<i32>,
      }

      if let Some(last_access_log) = AccessLogs::find()
        .select_only()
        .column(AccessLogsColumn::Count)
        .filter(AccessLogsColumn::UserId.eq(user_id))
        .filter(AccessLogsColumn::Method.eq(method.clone()))
        .filter(AccessLogsColumn::Code.eq(code.clone()))
        .into_model::<LastAccessLog>()
        .one(db)
        .await?
      {
        Some(last_access_log.count.unwrap_or(0) + 1)
      } else {
        Some(0)
      }
    } else {
      None
    };

    AccessLogs::insert(entity::access_logs::ActiveModel {
      user_id: Set(user_id.unwrap_or(self.profile.id)),
      method: Set(method),
      code: Set(code),
      count: Set(count),
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

  pub fn resolve_user_id(&self, explicit: Option<i32>) -> i32 {
    explicit.unwrap_or(self.profile.id)
  }

  pub async fn check_banned<C: ConnectionTrait>(
    &self,
    user_id: Option<i32>,
    db: &C,
  ) -> Result<bool, SumidaError> {
    let lhs = {
      let env = &self.env;
      let mut cond = Condition::any();

      // pub os_family:      String,
      // pub os_major:       Option<String>,
      // pub os_minor:       Option<String>,
      // pub os_patch:       Option<String>,
      // pub os_patch_minor: Option<String>,
      //
      // pub device_family: String,
      // pub device_brand:  Option<String>,
      // pub device_model:  Option<String>,

      cond = cond
        .add(
          Condition::any()
            .add(BansColumn::IpAddress.eq("*"))
            .add(BansColumn::IpAddress.eq(&env.ip_address)),
        )
        .add(
          Condition::any()
            .add(BansColumn::UserAgent.eq("*"))
            .add(BansColumn::UserAgent.eq(&env.user_agent)),
        )
        .add(
          Condition::any()
            .add(BansColumn::OsFamily.eq("*"))
            .add(BansColumn::OsFamily.eq(&env.os_family)),
        )
        .add(
          Condition::any()
            .add(BansColumn::DeviceFamily.eq("*"))
            .add(BansColumn::DeviceFamily.eq(&env.device_family)),
        );

      if let Some(user_id) = user_id {
        cond = cond.add(BansColumn::UserId.eq(user_id));
      }

      if let Some(ref os_major) = env.os_major {
        cond = cond.add(
          Condition::any() //
            .add(BansColumn::OsMajor.eq("*"))
            .add(BansColumn::OsMajor.eq(os_major)),
        );
      }

      if let Some(ref os_minor) = env.os_minor {
        cond = cond.add(
          Condition::any() //
            .add(BansColumn::OsMinor.eq("*"))
            .add(BansColumn::OsMinor.eq(os_minor)),
        );
      }

      if let Some(ref os_patch) = env.os_patch {
        cond = cond.add(
          Condition::any() //
            .add(BansColumn::OsPatch.eq("*"))
            .add(BansColumn::OsPatch.eq(os_patch)),
        );
      }

      if let Some(ref os_patch_minor) = env.os_patch_minor {
        cond = cond.add(
          Condition::any() //
            .add(BansColumn::OsPatchMinor.eq("*"))
            .add(BansColumn::OsPatchMinor.eq(os_patch_minor)),
        );
      }

      if let Some(ref device_brand) = env.device_brand {
        cond = cond.add(
          Condition::any() //
            .add(BansColumn::DeviceBrand.eq("*"))
            .add(BansColumn::DeviceBrand.eq(device_brand)),
        );
      }

      if let Some(ref device_model) = env.device_model {
        cond = cond.add(
          Condition::any() //
            .add(BansColumn::DeviceModel.eq("*"))
            .add(BansColumn::DeviceModel.eq(device_model)),
        );
      }

      cond
    };
    let rhs = {
      let now = Utc::now().fixed_offset();

      Condition::all() //
        .add(BansColumn::LiftedAt.is_null())
        .add(
          Condition::any() //
            .add(BansColumn::ExpiresAt.is_null())
            .add(BansColumn::ExpiresAt.gt(now)),
        )
    };
    let cond = Condition::all().add(lhs).add(rhs);

    Ok(Bans::find().filter(cond).count(db).await? > 0)
  }

  pub async fn ban_ip<C: ConnectionTrait>(
    &self,
    duration: Option<Duration>,
    reason: &str,
    note: Option<&str>,
    db: &C,
  ) -> Result<(), SumidaError> {
    let expires_at = duration.map(|x| (Utc::now() + x).fixed_offset());

    Bans::insert(entity::bans::ActiveModel {
      ip_address: Set(Some(self.env.ip_address.clone())),
      created_at: Set(Utc::now().fixed_offset()),
      reason: Set(Some(reason.to_string())),
      expires_at: Set(expires_at),
      note: Set(note.map(String::from)),
      ..Default::default()
    })
    .exec(db)
    .await?;

    Ok(())
  }

  pub async fn ban_account<C: ConnectionTrait>(
    &self,
    user_id: Option<i32>,
    duration: Option<Duration>,
    reason: &str,
    note: Option<&str>,
    db: &C,
  ) -> Result<(), SumidaError> {
    let user_id = self.resolve_user_id(user_id);
    let expires_at = duration.map(|x| (Utc::now() + x).fixed_offset());

    Bans::insert(entity::bans::ActiveModel {
      user_id: Set(Some(user_id)),
      created_at: Set(Utc::now().fixed_offset()),
      reason: Set(Some(reason.to_string())),
      expires_at: Set(expires_at),
      note: Set(note.map(String::from)),
      ..Default::default()
    })
    .exec(db)
    .await?;

    Ok(())
  }

  pub async fn ban_if_exceeded_limit<C: ConnectionTrait>(
    &mut self,
    user_id: Option<i32>,
    db: &C,
  ) -> Result<bool, SumidaError> {
    let settings = &settings::get().security.ban;
    // Do nothing if the ban feature is disabled.
    if !settings.enabled {
      return Ok(false);
    }
    let user_id = self.resolve_user_id(user_id);

    let Some(last_fail_log) = AccessLogs::find()
      .filter(AccessLogsColumn::UserId.eq(user_id))
      .filter(AccessLogsColumn::Method.eq(AccessMethod::Password))
      .filter(AccessLogsColumn::Code.eq(AccessResultCode::PasswordMismatch))
      .order_by_desc(AccessLogsColumn::CreatedAt)
      .one(db)
      .await?
    else {
      return Ok(false);
    };

    if AccessLogs::find()
      .filter(AccessLogsColumn::UserId.eq(user_id))
      .filter(AccessLogsColumn::Method.eq(AccessMethod::Password))
      .filter(AccessLogsColumn::Code.eq(AccessResultCode::Success))
      .filter(AccessLogsColumn::CreatedAt.gte(last_fail_log.created_at))
      .order_by_desc(AccessLogsColumn::CreatedAt)
      .one(db)
      .await?
      .is_some()
    {
      return Ok(false);
    }

    if (last_fail_log.count.unwrap_or(0) as u32) > settings.max_failed_attempts {
      let duration = humantime::parse_duration(&settings.failure_reset_after)
        .ok()
        .and_then(|x| Duration::from_std(x).ok());
      // The ban_ip/ban_account signature is (.., reason: &str, note: Option<&str>, ..).
      // Put the reason in reason and leave note empty (previously the positions were swapped, so reason was an empty string).
      let reason = "system banned";

      match settings.target.as_str() {
        "ip" => self.ban_ip(duration, reason, None, db).await?,
        "account" => self.ban_account(Some(user_id), duration, reason, None, db).await?,
        _ => {}
      };

      return Ok(true);
    }

    Ok(false)
  }

  pub fn verify(&self) -> Result<(), SumidaError> {
    if self.profile.is_authorized {
      Ok(())
    } else {
      Err(if self.profile.username.is_empty() {
        SumidaError::NoCredentials
      } else {
        SumidaError::NotAuthorized(self.profile.id)
      })
    }
  }

  pub async fn identify<C: ConnectionTrait + TransactionTrait>(
    &mut self,
    db: &C,
  ) -> Result<(), SumidaError> {
    if self.profile.is_authorized {
      let profile = &mut self.profile;
      let txn = db.begin().await?;

      Users::update(entity::users::ActiveModel {
        id: Unchanged(profile.id),
        username: Set(profile.username.clone()),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      let user = entity::users::Entity::find_by_id(profile.id)
        .one(&txn)
        .await?
        .ok_or(SumidaError::InvalidAccount)?;
      profile.update(user, &txn).await?;

      txn.commit().await?;
    } else {
      // Clean up the unauthenticated session. Since the subsequent store() saves the anonymous profile again,
      // here we only reset the profile rather than flush (kept separate from public logout).
      self.profile = UserProfile::default();
    }

    self.profile.last_seen_at = Utc::now().into();
    self.store().await?;

    Ok(())
  }

  pub async fn store(&self) -> Result<(), anyhow::Error> {
    self.inner.insert(Self::USER_PROFILE_KEY, self.profile.clone()).await?;
    self.inner.insert(Self::USER_ENV_KEY, self.env.clone()).await?;
    self.inner.insert(Self::USER_STATE_KEY, self.state.clone()).await?;
    self.inner.save().await?;

    Ok(())
  }

  pub async fn logout(&mut self) -> Result<(), anyhow::Error> {
    self.profile = UserProfile::default();
    // Completely destroy the session data and ID (overwriting only the profile would leave the session record/ID behind).
    self.inner.flush().await?;

    Ok(())
  }
}

impl<S> FromRequestParts<S> for UserSession
where
  S: Send + Sync,
{
  type Rejection = SumidaError;

  async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let inner = Session::from_request_parts(req, state)
      .await
      .map_err(|err| SumidaError::InternalServerError(err.1.to_string()))?;
    let profile = inner.get(Self::USER_PROFILE_KEY).await.ok().flatten().unwrap_or_default();
    let user_state = inner.get(Self::USER_STATE_KEY).await.ok().flatten().unwrap_or_default();
    let env = UserEnvironment::from_request_parts(req, state).await.unwrap_or_default();

    let mut session = Self {
      inner,
      profile,
      env,
      state: user_state,
    };
    session.profile.last_seen_at = Utc::now().into();
    // tower-sessions stores only the expiry "instant" in the record, while the "policy" (Expiry) is
    // reset every request to the layer default (here None=OnSessionEnd). If the save() below overwrites
    // it with that None policy, the remember-me/expiry set at login is lost (the cookie is downgraded to
    // a session cookie) and the session drops. So we re-apply profile.expiry saved at login on every request.
    session.inner.set_expiry(session.profile.expiry);
    session
      .inner
      .insert(Self::USER_PROFILE_KEY, session.profile.clone())
      .await
      .map_err(anyhow::Error::from)?;
    session.inner.save().await.map_err(anyhow::Error::from)?;

    Ok(session)
  }
}
