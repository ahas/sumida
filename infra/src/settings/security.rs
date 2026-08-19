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
#[cfg(feature = "system-settings")]
pub async fn resolve_ban_settings<C: sea_orm::ConnectionTrait>(db: &C) -> BanSettings {
  use entity::prelude::SystemSettings;
  use entity::system_settings::keys;

  let fallback = &crate::settings::get().security.ban;

  BanSettings {
    enabled:             read(
      keys::SECURITY_BAN_ENABLED,
      SystemSettings::get_bool(keys::SECURITY_BAN_ENABLED, db).await,
    )
    .unwrap_or(fallback.enabled),
    max_failed_attempts: read(
      keys::SECURITY_BAN_MAX_FAILED_ATTEMPTS,
      SystemSettings::get_i64(keys::SECURITY_BAN_MAX_FAILED_ATTEMPTS, db).await,
    )
    .and_then(|value| u32::try_from(value).ok())
    .unwrap_or(fallback.max_failed_attempts),
    failure_reset_after: read(
      keys::SECURITY_BAN_FAILURE_RESET_AFTER,
      SystemSettings::get_string(keys::SECURITY_BAN_FAILURE_RESET_AFTER, db).await,
    )
    .unwrap_or_else(|| fallback.failure_reset_after.clone()),
    duration:            read(
      keys::SECURITY_BAN_DURATION,
      SystemSettings::get_string(keys::SECURITY_BAN_DURATION, db).await,
    )
    .unwrap_or_else(|| fallback.duration.clone()),
    target:              read(
      keys::SECURITY_BAN_TARGET,
      SystemSettings::get_string(keys::SECURITY_BAN_TARGET, db).await,
    )
    .unwrap_or_else(|| fallback.target.clone()),
  }
}

/// Turn a store read into `Option`, warning when the store itself could not answer.
#[cfg(feature = "system-settings")]
fn read<T>(key: &str, result: Result<Option<T>, error::SumidaError>) -> Option<T> {
  match result {
    Ok(value) => value,
    Err(err) => {
      tracing::warn!("system setting `{key}` is unreadable, using the settings-file value: {err}");
      None
    }
  }
}
