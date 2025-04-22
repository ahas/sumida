use config::{Config, ConfigError, File};
use lazy_static::lazy_static;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SmtpSettings {
  pub username: String,
  pub password: String,
  pub server: String,
  pub auth: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionSettings {
  pub table: String,
  pub name: Option<String>,
  pub domain: Option<String>,
  pub path: Option<String>,
  pub same_site: Option<String>,
  pub secure: Option<bool>,
}

#[cfg(feature = "jwt")]
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JwtSettings {
  pub secret: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AwsS3Settings {
  pub bucket: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AwsSettings {
  pub region: Option<String>,
  pub s3: Option<AwsS3Settings>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DatabaseSettings {
  pub url: String,
  pub max_connections: Option<u32>,
  pub min_connections: Option<u32>,
  pub connect_timeout: Option<u64>,
  pub idle_timeout: Option<u64>,
  pub acquire_timeout: Option<u64>,
  pub max_lifetime: Option<u64>,
  pub logging: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Cors {
  pub allow_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ServerSettings {
  pub host: String,
  pub stage: String,
  pub logging: Option<String>,
  pub cors: Option<Cors>,
  pub cdn_url: Option<String>,
}

impl Default for ServerSettings {
  fn default() -> Self {
    Self {
      host: "0.0.0.0".to_string(),
      stage: "dev".to_string(),
      logging: Some("info".to_string()),
      cors: None,
      cdn_url: None,
    }
  }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SumidaSettings {
  pub server: ServerSettings,
  pub database: DatabaseSettings,
  pub smtp: Option<SmtpSettings>,
  #[cfg(feature = "aws")]
  pub aws: Option<AwsSettings>,
  #[cfg(feature = "session")]
  pub session: SessionSettings,
  #[cfg(feature = "jwt")]
  pub jwt: Option<JwtSettings>,
}

impl SumidaSettings {
  pub fn build<'de, T>() -> Result<T, ConfigError>
  where
    T: Deserialize<'de>,
  {
    let s = Config::builder().add_source(File::with_name("settings.toml")).build()?;

    s.try_deserialize()
  }
}

lazy_static! {
  pub static ref SUMIDA_SETTINGS: SumidaSettings = SumidaSettings::build().unwrap();
}

pub fn get() -> &'static SumidaSettings {
  &SUMIDA_SETTINGS
}
