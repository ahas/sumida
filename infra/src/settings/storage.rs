use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StorageSettings {
  pub local: Option<LocalStorageSettings>,
  pub s3: Option<S3StorageSettings>,
  /// Global limits applied to general-purpose `/files` uploads (board uploads follow per-board settings).
  #[serde(default)]
  pub upload: UploadSettings,
}

impl Default for StorageSettings {
  fn default() -> Self {
    Self {
      local: Some(LocalStorageSettings::default()),
      s3: None,
      upload: UploadSettings::default(),
    }
  }
}

/// General-purpose upload limits. If both are empty (0 / empty list) there is no size/extension restriction, but
/// `FileValidator` always blocks dangerous MIME types (executables/scripts).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct UploadSettings {
  /// Maximum file size (bytes). 0 means unlimited.
  pub max_file_size: usize,
  /// Allowed-extension whitelist (lowercase). If empty, all extensions are allowed.
  pub allowed_file_types: Vec<String>,
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
