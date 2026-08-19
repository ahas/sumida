use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DatabaseSettings {
  pub url:                    String,
  pub max_connections:        Option<u32>,
  pub min_connections:        Option<u32>,
  pub connect_timeout:        Option<String>,
  pub idle_timeout:           Option<String>,
  pub acquire_timeout:        Option<String>,
  pub acquire_slow_threshold: Option<String>,
  pub max_lifetime:           Option<String>,
  pub logging:                Option<String>,
}
