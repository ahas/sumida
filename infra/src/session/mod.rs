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
use sea_orm::{Condition, InsertResult, QueryOrder, QuerySelect, TransactionTrait};
use serde::{Deserialize, Serialize};
use tower_sessions::{Expiry, Session};
use uaparser::{Parser, UserAgentParser};

use crate::settings;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
  pub id:            i32,
  pub username:      String,
  pub full_name:     String,
  pub organization:  UserOrganization,
  pub role:          UserRole,
  pub is_admin:      bool,
  pub is_authorized: bool,
  pub api_key:       Option<String>,
  pub first_seen_at: DateTimeUtc,
  pub last_seen_at:  DateTimeUtc,
  pub expiry:        Option<Expiry>,
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
      id:   organization.id,
      name: organization.name.clone(),
    };
    self.role = UserRole {
      id:   role.id,
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
      id:            0,
      username:      "".to_owned(),
      full_name:     "".to_owned(),
      organization:  UserOrganization::default(),
      role:          UserRole::default(),
      is_admin:      false,
      is_authorized: false,
      api_key:       None,
      first_seen_at: Utc::now(),
      last_seen_at:  Utc::now(),
      expiry:        None,
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
  pub id:   i32,
  pub name: String,
}

impl Default for UserOrganization {
  fn default() -> Self {
    Self {
      id:   0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRole {
  pub id:   i32,
  pub name: String,
}

impl Default for UserRole {
  fn default() -> Self {
    Self {
      id:   0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserEnvironment {
  pub ip_address: String,
  pub user_agent: String,

  pub os_family:      String,
  pub os_major:       Option<String>,
  pub os_minor:       Option<String>,
  pub os_patch:       Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand:  Option<String>,
  pub device_model:  Option<String>,
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
      ip_address:     ip.to_string(),
      user_agent:     user_agent.clone(),
      os_family:      ua_os.family.to_string(),
      os_major:       ua_os.major.map(|x| x.to_string()),
      os_minor:       ua_os.minor.map(|x| x.to_string()),
      os_patch:       ua_os.patch.map(|x| x.to_string()),
      os_patch_minor: ua_os.patch_minor.map(|x| x.to_string()),
      device_family:  ua_device.family.to_string(),
      device_brand:   ua_device.brand.map(|x| x.to_string()),
      device_model:   ua_device.model.map(|x| x.to_string()),
    })
  }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserState {}

/// Whether a live ban catches this attempt.
///
/// A ban row states a set of conditions that must **all** hold for it to catch a request: whom it
/// targets, and which environment it targets. Every environment column defaults to the wildcard, so
/// a row naming only an account leaves the rest open — which is what makes the conjunction the
/// load-bearing part. Read as a disjunction, one open column would be enough to catch every
/// request, and banning one account would lock out everybody.
///
/// This takes the account and the environment rather than a session, because those are the only
/// things it reads. A session cannot be stood up without a request, and a rule this consequential
/// has to be checkable without one.
pub async fn is_banned<C: ConnectionTrait>(
  user_id: Option<i32>,
  env: &UserEnvironment,
  db: &C,
) -> Result<bool, SumidaError> {
  // A column catches when the row left it open — the wildcard, or nothing at all — or when it names
  // exactly what this request carries. A request carrying no value for a column is caught only by a
  // row that left the column open.
  fn column(col: BansColumn, value: Option<&str>) -> Condition {
    let open = Condition::any().add(col.eq("*")).add(col.is_null());

    match value {
      Some(value) => open.add(col.eq(value)),
      None => open,
    }
  }

  let target = match user_id {
    // An environment ban names no account and catches whoever arrives; an account ban catches only
    // that account.
    Some(user_id) => Condition::any()
      .add(BansColumn::UserId.is_null())
      .add(BansColumn::UserId.eq(user_id)),
    // An attempt that never resolved to an account is caught by an environment ban alone.
    None => Condition::all().add(BansColumn::UserId.is_null()),
  };

  let matches = Condition::all()
    .add(target)
    .add(column(BansColumn::IpAddress, Some(&env.ip_address)))
    .add(column(BansColumn::UserAgent, Some(&env.user_agent)))
    .add(column(BansColumn::OsFamily, Some(&env.os_family)))
    .add(column(BansColumn::OsMajor, env.os_major.as_deref()))
    .add(column(BansColumn::OsMinor, env.os_minor.as_deref()))
    .add(column(BansColumn::OsPatch, env.os_patch.as_deref()))
    .add(column(BansColumn::OsPatchMinor, env.os_patch_minor.as_deref()))
    .add(column(BansColumn::DeviceFamily, Some(&env.device_family)))
    .add(column(BansColumn::DeviceBrand, env.device_brand.as_deref()))
    .add(column(BansColumn::DeviceModel, env.device_model.as_deref()));

  // A disposition that was lifted, or whose term has run out, catches nothing.
  let live = Condition::all()
    .add(BansColumn::LiftedAt.is_null())
    .add(
      Condition::any()
        .add(BansColumn::ExpiresAt.is_null())
        .add(BansColumn::ExpiresAt.gt(Utc::now())),
    );

  Ok(Bans::find().filter(Condition::all().add(matches).add(live)).count(db).await? > 0)
}

/// The account behind a credential, or the reason it is refused.
///
/// Every way in asks this — a password sign-in, a bearer token, an api key, a session cookie —
/// because what it asks about is the account, not the credential that arrived with the request. An
/// account disabled or banned after a credential was handed out is refused from the next request
/// on rather than from its next sign-in, so no credential can carry the answer with it.
pub async fn admitted_account<C: ConnectionTrait>(
  user_id: i32,
  env: &UserEnvironment,
  db: &C,
) -> Result<entity::users::Model, SumidaError> {
  let Some(user) = Users::find_by_id(user_id).one(db).await? else {
    return Err(SumidaError::InvalidAccount);
  };

  if !user.is_enabled {
    return Err(SumidaError::InactiveAccount);
  }

  if is_banned(Some(user.id), env, db).await? {
    return Err(SumidaError::AccountBanned);
  }

  Ok(user)
}

/// Whether this account has reset its credentials since one established at `established_at`.
///
/// A password is changed because what was handed out before it may be in somebody else's hands,
/// and a token or a cookie cannot be told apart from the same one held by whoever took it. So the
/// moment of the reset voids every credential established up to it. A token records its own moment
/// only to the second, and one minted during that second goes with the rest rather than surviving
/// on the rounding.
pub fn credentials_reset_since(
  user: &entity::users::Model,
  established_at: DateTimeUtc,
) -> bool {
  user.credentials_reset_at.is_some_and(|reset_at| established_at <= reset_at)
}

#[derive(Clone, Debug)]
pub struct UserSession {
  pub inner:   Session,
  pub profile: UserProfile,
  pub env:     UserEnvironment,
  pub state:   UserState,
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
    self.profile.first_seen_at = Utc::now();

    self.inner.set_expiry(expiry);
    self.identify(db).await?;

    // Session fixation defense: reissue the session ID on login.
    // (Prevents a pre-auth anonymous session ID from continuing to be used after authentication.)
    self.inner.cycle_id().await.map_err(anyhow::Error::from)?;

    Ok(())
  }

  /// Records an attempt, unless it resolved to no account.
  ///
  /// The column naming the account is a key into the accounts themselves, so there is no value in
  /// it standing for nobody: a username no account answers to has nothing to write there. The
  /// attempt is refused on its own terms, and the refusal is what the caller is answered with —
  /// the write is not allowed to turn a turned-away sign-in into a failure of the server.
  pub async fn write_log(
    &self,
    user_id: Option<i32>,
    method: AccessMethod,
    code: AccessResultCode,
    db: &DatabaseConnection,
  ) -> Result<Option<InsertResult<entity::access_logs::ActiveModel>>, DbErr> {
    let Some(user_id) = user_id.or_else(|| self.profile.get_id()) else {
      return Ok(None);
    };

    // `count` is a plain fact about the row being written, with no policy in it: how many password
    // failures this account has run up since its last successful password sign-in, this one
    // included. Nothing decides anything from it — the lockout counts the log itself — but a column
    // that states a number has to state the right one.
    let count: Option<i32> = if matches!(code, AccessResultCode::PasswordMismatch) {
      let earlier = password_failures_since_success(user_id, db).await?;
      Some(i32::try_from(earlier.saturating_add(1)).unwrap_or(i32::MAX))
    } else {
      None
    };

    let written = AccessLogs::insert(entity::access_logs::ActiveModel {
      user_id: Set(user_id),
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
    .await?;

    Ok(Some(written))
  }

  pub fn resolve_user_id(&self, explicit: Option<i32>) -> i32 {
    explicit.unwrap_or(self.profile.id)
  }

  pub async fn check_banned<C: ConnectionTrait>(
    &self,
    user_id: Option<i32>,
    db: &C,
  ) -> Result<bool, SumidaError> {
    is_banned(user_id, &self.env, db).await
  }

  pub async fn admitted_account<C: ConnectionTrait>(
    &self,
    user_id: i32,
    db: &C,
  ) -> Result<entity::users::Model, SumidaError> {
    admitted_account(user_id, &self.env, db).await
  }

  pub async fn ban_ip<C: ConnectionTrait>(
    &self,
    duration: Option<Duration>,
    reason: &str,
    note: Option<&str>,
    db: &C,
  ) -> Result<(), SumidaError> {
    let expires_at = duration.map(|x| Utc::now() + x);

    Bans::insert(entity::bans::ActiveModel {
      ip_address: Set(Some(self.env.ip_address.clone())),
      created_at: Set(Utc::now()),
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
    let expires_at = duration.map(|x| Utc::now() + x);

    Bans::insert(entity::bans::ActiveModel {
      user_id: Set(Some(user_id)),
      created_at: Set(Utc::now()),
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
    // The policy is an operational setting: read it from the system settings store when that
    // feature is on, and from the settings file otherwise.
    #[cfg(feature = "system-settings")]
    let settings = &settings::security::resolve_ban_settings(db).await;
    #[cfg(not(feature = "system-settings"))]
    let settings = &settings::get().security.ban;
    // Do nothing if the ban feature is disabled.
    if !settings.enabled {
      return Ok(false);
    }
    // A threshold of zero would be met by the shortest run there is and block on sight. Nobody
    // setting it means that, so it is refused rather than arrived at by arithmetic.
    if settings.max_failed_attempts == 0 {
      return Ok(false);
    }

    let user_id = self.resolve_user_id(user_id);
    let allowed = u64::from(settings.max_failed_attempts);

    // This runs after the attempt that brought us here has already been written, so the run read
    // back includes it. Reading no more rows than the threshold answers the only question asked —
    // has the run reached the allowed number — and leaves nothing unbounded on the path an attacker
    // drives hardest.
    let reset_after =
      settings::security::parse_span("security.ban.failure_reset_after", &settings.failure_reset_after);
    let failures = recent_password_failures(user_id, allowed, db).await?;

    if u64::from(failure_streak(&failures, reset_after)) < allowed {
      return Ok(false);
    }

    let expires_in = settings::security::parse_span("security.ban.duration", &settings.duration);
    let reason = "system banned";

    match settings.target.as_str() {
      "ip" => self.ban_ip(expires_in, reason, None, db).await?,
      "account" => self.ban_account(Some(user_id), expires_in, reason, None, db).await?,
      other => {
        tracing::warn!(
          "`security.ban.target` is `{other}`, which names nothing that can be banned; nothing was banned"
        );
        return Ok(false);
      }
    }

    Ok(true)
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

    self.profile.last_seen_at = Utc::now();
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
    session.profile.last_seen_at = Utc::now();
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

/// The moment of this account's most recent successful password sign-in, if it has ever had one.
///
/// A success ends a run of failures — what came before it is not consecutive with what comes after
/// — so it is where every count on this path stops looking back.
async fn last_password_success<C: ConnectionTrait>(
  user_id: i32,
  db: &C,
) -> Result<Option<DateTimeUtc>, DbErr> {
  AccessLogs::find()
    .select_only()
    .column(AccessLogsColumn::CreatedAt)
    .filter(AccessLogsColumn::UserId.eq(user_id))
    .filter(AccessLogsColumn::Method.eq(AccessMethod::Password))
    .filter(AccessLogsColumn::Code.eq(AccessResultCode::Success))
    .order_by_desc(AccessLogsColumn::CreatedAt)
    .into_tuple::<DateTimeUtc>()
    .one(db)
    .await
}

/// How many password failures this account has run up since its last successful password sign-in.
async fn password_failures_since_success<C: ConnectionTrait>(user_id: i32, db: &C) -> Result<u64, DbErr> {
  let mut query = AccessLogs::find()
    .filter(AccessLogsColumn::UserId.eq(user_id))
    .filter(AccessLogsColumn::Method.eq(AccessMethod::Password))
    .filter(AccessLogsColumn::Code.eq(AccessResultCode::PasswordMismatch));

  if let Some(since) = last_password_success(user_id, db).await? {
    query = query.filter(AccessLogsColumn::CreatedAt.gt(since));
  }

  query.count(db).await
}

/// The moments of this account's most recent password failures, newest first, none of them older
/// than its last successful password sign-in.
///
/// `limit` caps the read. The caller only asks whether the run has reached the allowed number, and
/// no row beyond that many can change the answer.
async fn recent_password_failures<C: ConnectionTrait>(
  user_id: i32,
  limit: u64,
  db: &C,
) -> Result<Vec<DateTimeUtc>, DbErr> {
  let mut query = AccessLogs::find()
    .select_only()
    .column(AccessLogsColumn::CreatedAt)
    .filter(AccessLogsColumn::UserId.eq(user_id))
    .filter(AccessLogsColumn::Method.eq(AccessMethod::Password))
    .filter(AccessLogsColumn::Code.eq(AccessResultCode::PasswordMismatch));

  if let Some(since) = last_password_success(user_id, db).await? {
    query = query.filter(AccessLogsColumn::CreatedAt.gt(since));
  }

  query
    .order_by_desc(AccessLogsColumn::CreatedAt)
    .limit(limit)
    .into_tuple::<DateTimeUtc>()
    .all(db)
    .await
}

/// How many of `newest_first` form one unbroken run of failures.
///
/// The walk starts at the newest and stops at the first gap between neighbours wider than
/// `reset_after`: that gap is the window passing with nothing in it, which is where counting starts
/// again from zero. With no window nothing breaks the run, and the whole slice is one streak.
fn failure_streak(newest_first: &[DateTimeUtc], reset_after: Option<Duration>) -> u32 {
  let Some((first, rest)) = newest_first.split_first() else {
    return 0;
  };

  let mut newer = first;
  let mut streak = 1;

  for older in rest {
    if let Some(window) = reset_after
      && *newer - *older > window
    {
      break;
    }

    newer = older;
    streak += 1;
  }

  streak
}

#[cfg(test)]
mod tests {
  use chrono::{Duration, TimeZone, Utc};
  use sea_orm::prelude::DateTimeUtc;

  use super::failure_streak;

  /// Failure moments the given number of minutes back from a fixed point, newest first.
  fn minutes_ago(offsets: &[i64]) -> Vec<DateTimeUtc> {
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    offsets.iter().map(|offset| base - Duration::minutes(*offset)).collect()
  }

  #[test]
  fn an_empty_log_is_no_streak() {
    assert_eq!(failure_streak(&[], Some(Duration::minutes(10))), 0);
    assert_eq!(failure_streak(&[], None), 0);
  }

  #[test]
  fn a_single_failure_is_a_streak_of_one() {
    assert_eq!(failure_streak(&minutes_ago(&[0]), Some(Duration::minutes(10))), 1);
  }

  #[test]
  fn neighbours_inside_the_window_are_one_run() {
    assert_eq!(failure_streak(&minutes_ago(&[0, 5, 10, 15]), Some(Duration::minutes(10))), 4);
  }

  #[test]
  fn a_gap_wider_than_the_window_ends_the_run() {
    // The three newest sit five minutes apart; the fourth is an hour further back, so the run
    // ends before it and the fifth is out of reach behind it.
    assert_eq!(failure_streak(&minutes_ago(&[0, 5, 10, 70, 75]), Some(Duration::minutes(10))), 3);
  }

  #[test]
  fn with_no_window_nothing_breaks_the_run() {
    assert_eq!(failure_streak(&minutes_ago(&[0, 5, 10, 70, 6000]), None), 5);
  }
}
