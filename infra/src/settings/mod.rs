#[cfg(feature = "aws")]
pub mod aws;
pub mod database;
pub mod jwt;
pub mod security;
pub mod server;
pub mod session;
pub mod smtp;
pub mod storage;

use config::{Config, File};
use lazy_static::lazy_static;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct SumidaSettings {
  pub server:   server::ServerSettings,
  pub database: database::DatabaseSettings,
  pub session:  session::SessionSettings,
  #[serde(default)]
  pub security: security::SecuritySettings,
  #[serde(default)]
  pub storage:  storage::StorageSettings,
  pub smtp:     Option<smtp::SmtpSettings>,
  pub jwt:      Option<jwt::JwtSettings>,
  #[cfg(feature = "aws")]
  pub aws:      Option<aws::AwsSettings>,
}

impl SumidaSettings {
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
    let source = File::from(std::path::PathBuf::from(settings_path));
    let config = Config::builder().add_source(source).build()?;

    Ok(config.try_deserialize()?)
  }
}

lazy_static! {
  pub static ref SUMIDA_SETTINGS: SumidaSettings = SumidaSettings::build().unwrap();
}

pub fn get() -> &'static SumidaSettings {
  &SUMIDA_SETTINGS
}
