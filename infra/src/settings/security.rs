use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct SecuritySettings {
  #[serde(default)]
  pub ban: BanSettings,
}

#[derive(Debug, Deserialize)]
pub struct BanSettings {
  pub enabled:             bool,
  pub max_failed_attempts: u32,
  pub failure_reset_after: String,
  pub duration:            String,
  pub target:              String,
}

impl Default for BanSettings {
  fn default() -> Self {
    Self {
      enabled:             true,
      max_failed_attempts: 10,
      failure_reset_after: "none".to_owned(),
      duration:            "forever".to_owned(),
      target:              "ip".to_owned(),
    }
  }
}

/// Resolve the ban policy from the system settings store, per field, falling back to the
/// settings-file value.
///
/// A missing row, a value of the wrong type, and a failed query all mean "no answer" and take the
/// fallback. A database problem must never break the login path, so a failed query is logged at
/// warn level and nothing else. This is read at the point of use rather than cached at boot —
/// caching would hide an operator's edit until the process is rebuilt.
///
/// The five rows are fetched together. This runs on the failed-login path, which is exactly the
/// one an attacker drives hardest, so it costs one round trip rather than five — and when the
/// store is unreachable it says so once instead of five times.
#[cfg(feature = "system-settings")]
pub async fn resolve_ban_settings<C: sea_orm::ConnectionTrait>(db: &C) -> BanSettings {
  use std::collections::HashMap;

  use entity::prelude::{SystemSettings, SystemSettingsColumn};
  use entity::system_settings::keys;
  use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

  let fallback = &crate::settings::get().security.ban;

  let keys_wanted = [
    keys::SECURITY_BAN_ENABLED,
    keys::SECURITY_BAN_MAX_FAILED_ATTEMPTS,
    keys::SECURITY_BAN_FAILURE_RESET_AFTER,
    keys::SECURITY_BAN_DURATION,
    keys::SECURITY_BAN_TARGET,
  ];

  let stored: HashMap<String, sea_orm::JsonValue> = match SystemSettings::find()
    .filter(SystemSettingsColumn::Key.is_in(keys_wanted))
    .all(db)
    .await
  {
    Ok(rows) => rows.into_iter().map(|row| (row.key, row.value)).collect(),
    Err(err) => {
      tracing::warn!("the system settings store is unreadable, using the settings-file ban policy: {err}");
      HashMap::new()
    }
  };

  BanSettings {
    enabled:             stored
      .get(keys::SECURITY_BAN_ENABLED)
      .and_then(|value| value.as_bool())
      .unwrap_or(fallback.enabled),
    max_failed_attempts: stored
      .get(keys::SECURITY_BAN_MAX_FAILED_ATTEMPTS)
      .and_then(|value| value.as_i64())
      .and_then(|value| u32::try_from(value).ok())
      .unwrap_or(fallback.max_failed_attempts),
    failure_reset_after: stored
      .get(keys::SECURITY_BAN_FAILURE_RESET_AFTER)
      .and_then(|value| value.as_str())
      .map_or_else(|| fallback.failure_reset_after.clone(), String::from),
    duration:            stored
      .get(keys::SECURITY_BAN_DURATION)
      .and_then(|value| value.as_str())
      .map_or_else(|| fallback.duration.clone(), String::from),
    target:              stored
      .get(keys::SECURITY_BAN_TARGET)
      .and_then(|value| value.as_str())
      .map_or_else(|| fallback.target.clone(), String::from),
  }
}
