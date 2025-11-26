use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct SecuritySettings {
  #[serde(default)]
  pub ban: BanSettings,
}

#[derive(Debug, Deserialize)]
pub struct BanSettings {
  pub enabled: bool,
  pub max_failed_attempts: u32,
  pub failure_reset_after: String,
  pub duration: String,
  pub target: String,
}

impl Default for BanSettings {
  fn default() -> Self {
    Self {
      enabled: true,
      max_failed_attempts: 10,
      failure_reset_after: "none".to_owned(),
      duration: "forever".to_owned(),
      target: "ip".to_owned(),
    }
  }
}
