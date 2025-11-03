use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StorageSettings {
  pub local: Option<LocalStorageSettings>,
  pub s3: Option<S3StorageSettings>,
}

impl Default for StorageSettings {
  fn default() -> Self {
    Self {
      local: Some(LocalStorageSettings::default()),
      s3: None,
    }
  }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocalStorageSettings {
  pub directory: String,
}

impl Default for LocalStorageSettings {
  fn default() -> Self {
    Self {
      directory: "./uploads".to_string(),
    }
  }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct S3StorageSettings {
  pub bucket: String,
  pub prefix: Option<String>,
  pub region: Option<String>,
}
