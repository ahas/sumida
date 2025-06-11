use serde::Deserialize;

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

