use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
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
      host: "0.0.0.0:0".to_string(),
      stage: "dev".to_string(),
      logging: Some("info".to_string()),
      cors: None,
      cdn_url: None,
    }
  }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CorsSettings {
  pub allow_origins: Vec<String>,
}
