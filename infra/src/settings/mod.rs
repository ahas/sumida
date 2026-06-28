#[cfg(feature = "aws")]
pub mod aws;
pub mod database;
pub mod jwt;
pub mod membership;
pub mod security;
pub mod server;
pub mod session;
pub mod smtp;
pub mod storage;

use figment::Figment;
use figment::providers::{Env, Format, Toml};
use lazy_static::lazy_static;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct SumidaSettings {
  pub server: server::ServerSettings,
  pub database: database::DatabaseSettings,
  pub session: session::SessionSettings,
  #[serde(default)]
  pub security: security::SecuritySettings,
  #[serde(default)]
  pub storage: storage::StorageSettings,
  #[serde(default)]
  pub membership: membership::MembershipSettings,
  pub smtp: Option<smtp::SmtpSettings>,
  pub jwt: Option<jwt::JwtSettings>,
  #[cfg(feature = "aws")]
  pub aws: Option<aws::AwsSettings>,
}

impl SumidaSettings {
  /// Load settings with precedence `toml(SUMIDA_SETTINGS) < SUMIDA_* env`.
  ///
  /// Env keys map to the (kebab-case) config keys: `__` separates nesting and a single `_` becomes
  /// `-` within a segment — e.g. `SUMIDA_STORAGE__ORPHAN_GRACE_HOURS` → `storage.orphan-grace-hours`,
  /// `SUMIDA_DATABASE__URL` → `database.url`. `SUMIDA_SETTINGS` (the file selector) is ignored.
  pub fn build<'de, T>() -> anyhow::Result<T>
  where
    T: Deserialize<'de>,
  {
    let settings_file = std::env::var("SUMIDA_SETTINGS").unwrap_or_else(|_| "settings.toml".to_string());
    let settings_path = if std::path::Path::new(&settings_file).is_absolute() {
      settings_file
    } else {
      std::env::current_dir()?.join(settings_file).to_string_lossy().to_string()
    };

    let settings = Figment::new()
      .merge(Toml::file(&settings_path))
      .merge(sumida_env().split("."))
      .extract()?;

    Ok(settings)
  }
}

lazy_static! {
  pub static ref SUMIDA_SETTINGS: SumidaSettings = SumidaSettings::build()
    .expect("failed to load settings: check the settings file/env vars (SUMIDA_*) and required keys");
}

pub fn get() -> &'static SumidaSettings {
  &SUMIDA_SETTINGS
}

/// `SUMIDA_*` env provider. Keys map to (kebab-case) config keys: `__` separates nesting and a single
/// `_` becomes `-` within a segment. Pair with `.split(".")` when merging. `SUMIDA_SETTINGS` is ignored.
fn sumida_env() -> Env {
  Env::prefixed("SUMIDA_").ignore(&["SETTINGS"]).map(|key| {
    key
      .as_str()
      .split("__")
      .map(|segment| segment.replace('_', "-"))
      .collect::<Vec<_>>()
      .join(".")
      .into()
  })
}

#[cfg(test)]
mod tests {
  use figment::providers::{Format, Toml};
  use figment::{Figment, Jail};
  use serde::Deserialize;

  use super::sumida_env;

  #[derive(Deserialize)]
  #[serde(rename_all = "kebab-case")]
  struct Storage {
    orphan_grace_hours: i64,
    max_file_size: u64,
  }

  #[derive(Deserialize)]
  struct Root {
    storage: Storage,
    database: Database,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "kebab-case")]
  struct Database {
    url: String,
  }

  #[test]
  #[allow(clippy::result_large_err)] // figment::Error (the Jail closure's Err) is large; fine in a test.
  fn env_overrides_toml_with_kebab_and_nesting() {
    Jail::expect_with(|jail| {
      jail.create_file(
        "s.toml",
        r#"
          [database]
          url = "postgres://from-file"

          [storage]
          orphan-grace-hours = 24
          max-file-size = 10
        "#,
      )?;
      // `__` nests, `_` -> `-` within a segment.
      jail.set_env("SUMIDA_STORAGE__ORPHAN_GRACE_HOURS", "1");
      jail.set_env("SUMIDA_DATABASE__URL", "postgres://from-env");

      let root: Root = Figment::new()
        .merge(Toml::file("s.toml"))
        .merge(sumida_env().split("."))
        .extract()
        .unwrap();

      // env wins over the file...
      assert_eq!(root.storage.orphan_grace_hours, 1);
      assert_eq!(root.database.url, "postgres://from-env");
      // ...and file values without an env override are kept.
      assert_eq!(root.storage.max_file_size, 10);
      Ok(())
    });
  }
}
