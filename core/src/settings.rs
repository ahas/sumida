use config::{Config, ConfigError, File};
use lazy_static::lazy_static;
use serde::Deserialize;
use time::OffsetDateTime;
use tower_sessions::Expiry;

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
  pub expiry: SessionExpirySettings,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionExpirySettings {
  default: String,
  remember_me: String,
}

impl SessionExpirySettings {
  pub fn default_expiry(&self) -> anyhow::Result<Expiry> {
    let duration = humantime::parse_duration(&self.default)?;
    Ok(Expiry::OnInactivity(time::Duration::try_from(duration).unwrap()))
  }

  pub fn remember_me_expiry(&self) -> anyhow::Result<Expiry> {
    let duration = humantime::parse_duration(&self.remember_me)?;
    Ok(Expiry::AtDateTime(OffsetDateTime::now_utc() + duration))
  }
}

impl Default for SessionExpirySettings {
  fn default() -> Self {
    Self {
      default: "24hours".to_owned(),
      remember_me: "1month".to_owned(),
    }
  }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JwtSettings {
  pub secret: Option<String>,
  pub expiry: JwtExpirySettings,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JwtExpirySettings {
  access_token: String,
  refresh_token: String,
  remember_me: String,
}

impl JwtExpirySettings {
  pub fn access_token_expiry(&self) -> anyhow::Result<Expiry> {
    let duration = humantime::parse_duration(&self.access_token)?;
    Ok(Expiry::AtDateTime(OffsetDateTime::now_utc() + duration))
  }

  pub fn refresh_token_expiry(&self) -> anyhow::Result<Expiry> {
    let duration = humantime::parse_duration(&self.access_token)?;
    Ok(Expiry::AtDateTime(OffsetDateTime::now_utc() + duration))
  }

  pub fn remember_me_expiry(&self) -> anyhow::Result<Expiry> {
    let duration = humantime::parse_duration(&self.access_token)?;
    Ok(Expiry::AtDateTime(OffsetDateTime::now_utc() + duration))
  }
}

impl Default for JwtExpirySettings {
  fn default() -> Self {
    Self {
      access_token: "1hour".to_owned(),
      refresh_token: "1week".to_owned(),
      remember_me: "1month".to_owned(),
    }
  }
}

#[cfg(feature = "aws-s3")]
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AwsS3Settings {
  pub bucket: String,
}

#[cfg(feature = "aws")]
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
pub struct CorsSettings {
  pub allow_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WebPushSettings {
  pub audience: String,
  pub expiration: String,
  pub subject: String,
  pub private_key: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ServerSettings {
  pub host: String,
  pub stage: String,
  pub logging: Option<String>,
  pub cors: Option<CorsSettings>,
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
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct SumidaSettings {
  pub server: ServerSettings,
  pub database: DatabaseSettings,
  pub session: SessionSettings,
  pub smtp: Option<SmtpSettings>,
  pub jwt: Option<JwtSettings>,
  pub web_push: WebPushSettings,
  #[cfg(feature = "aws")]
  pub aws: Option<AwsSettings>,
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

lazy_static! {
  pub static ref WEB_PUSH_PRIVATE_KEY_PEM: jsonwebtoken::EncodingKey = {
    let pem = std::fs::read_to_string(std::path::Path::new(&SUMIDA_SETTINGS.web_push.private_key))
      .expect("failed to read web-push private key file");
    let key = jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes())
      .expect("failed to parse web-push private key file to jwt encoding key");
    key
  };
}

pub fn get() -> &'static SumidaSettings {
  &SUMIDA_SETTINGS
}
