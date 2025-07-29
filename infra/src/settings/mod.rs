#[cfg(feature = "aws")]
pub mod aws;
pub mod database;
pub mod jwt;
pub mod security;
pub mod server;
pub mod session;
pub mod smtp;
pub mod web_push;

use config::{Config, ConfigError, File};
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
  pub smtp: Option<smtp::SmtpSettings>,
  pub jwt: Option<jwt::JwtSettings>,
  pub web_push: web_push::WebPushSettings,
  #[cfg(feature = "aws")]
  pub aws: Option<aws::AwsSettings>,
}

impl SumidaSettings {
  pub fn build<'de, T>() -> Result<T, ConfigError>
  where
    T: Deserialize<'de>,
  {
    let p = std::env::var("SUMIDA_SETTINGS").unwrap_or("settings.toml");
    let s = Config::builder().add_source(File::with_name(p)).build()?;

    s.try_deserialize()
  }
}

lazy_static! {
  pub static ref SUMIDA_SETTINGS: SumidaSettings = SumidaSettings::build().unwrap();
}

pub fn get() -> &'static SumidaSettings {
  &SUMIDA_SETTINGS
}
