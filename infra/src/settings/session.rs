use axum_client_ip::ClientIpSource;
use serde::Deserialize;
use time::OffsetDateTime;
use tower_sessions::Expiry;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionSettings {
  #[cfg(feature = "session-store-redis")]
  pub redis: Option<SessionRedisSettings>,
  #[cfg(feature = "session-store-aws-dynamodb")]
  pub dynamodb: Option<SessionDynamodbSettings>,

  pub ip_source: ClientIpSource,
  pub cookie: SessionCookieSettings,
}

#[cfg(feature = "session-store-redis")]
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionRedisSettings {
  pub url: Option<String>,
  pub size: Option<usize>,
}

#[cfg(feature = "session-store-aws-dynamodb")]
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionDynamodbSettings {
  pub table: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionCookieSettings {
  pub name: Option<String>,
  pub domain: Option<String>,
  pub path: Option<String>,
  pub same_site: Option<String>,
  pub secure: Option<bool>,
  #[serde(default)]
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
