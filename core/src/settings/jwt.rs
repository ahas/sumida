use serde::Deserialize;
use time::OffsetDateTime;
use tower_sessions::Expiry;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JwtSettings {
  pub secret: Option<String>,
  #[serde(default)]
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
