use std::path::PathBuf;
use std::pin::Pin;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::Response;
use error::SumidaError;
use infra::settings;
use tokio::io::AsyncRead;

use crate::util::file_validation::{FileInfo, IntoFileInfo};
#[cfg(feature = "storage-aws-s3")]
use crate::util::s3;

pub type FileStream = Pin<Box<dyn AsyncRead + Send>>;

/// RFC 5987 `ext-value` encoding (for Content-Disposition `filename*`). Leaves attr-char as-is
/// and percent-encodes the rest as `%HH`.
fn percent_encode_rfc5987(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for &b in s.as_bytes() {
    match b {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
      _ => out.push_str(&format!("%{b:02X}")),
    }
  }
  out
}

#[derive(Clone, Debug)]
pub struct StaticFile {
  pub name:          String,
  pub original_name: String,
  pub dir:           String,
  pub mime_type:     String,
  pub bytes:         Vec<u8>,
  pub extension:     Option<String>,
}

impl StaticFile {
  pub fn get_path(&self) -> String {
    std::path::Path::new(&self.dir)
      .join(std::path::Path::new(&self.name))
      .to_string_lossy()
      .into_owned()
  }
}

impl IntoFileInfo for StaticFile {
  fn into_file_info(&self) -> FileInfo {
    FileInfo::new(self.name.clone(), self.bytes.len(), self.mime_type.clone())
  }
}

#[derive(Clone, Debug)]
pub struct DownloadFile {
  pub name:          String,
  pub original_name: String,
  pub path:          String,
  pub mime_type:     String,
}

#[cfg(feature = "file")]
impl From<entity::fs::files::Model> for DownloadFile {
  fn from(value: entity::fs::files::Model) -> Self {
    Self {
      path:          value.get_path(),
      name:          value.name,
      original_name: value.original_name,
      mime_type:     value.mime_type,
    }
  }
}

#[async_trait]
pub trait Storage: Send + Sync {
  async fn store(&self, file: StaticFile) -> Result<(), SumidaError>;
  async fn delete(&self, path: &str) -> Result<(), SumidaError>;
  async fn stream(&self, path: &str) -> Result<FileStream, SumidaError>;
  async fn download(
    &self,
    file_data: DownloadFile,
    with_original_name: bool,
  ) -> Result<Response<Body>, SumidaError> {
    use axum::body::Body;
    use axum::http::{StatusCode, header};
    use tokio_util::io::ReaderStream;

    let file_path = file_data.path;
    let file_name = if with_original_name { file_data.original_name } else { file_data.name };
    let content_type = file_data.mime_type;
    let file_stream = self.stream(&file_path).await?;
    let file_size = self.get_file_size(&file_path).await?;

    let stream = ReaderStream::new(file_stream);
    let body = Body::from_stream(stream);

    // RFC 6266/5987: provide both an ASCII fallback (dangerous characters stripped) and the full UTF-8 representation.
    // file_name is the user-controlled original_name received at upload, so interpolating quotes/control characters
    // as-is allows header injection and filename spoofing.
    let ascii_fallback: String = file_name
      .chars()
      .filter(|c| c.is_ascii() && !c.is_ascii_control() && *c != '"' && *c != '\\')
      .collect();
    let content_disposition = format!(
      "attachment; filename=\"{ascii_fallback}\"; filename*=UTF-8''{}",
      percent_encode_rfc5987(&file_name)
    );

    let response = Response::builder()
      .status(StatusCode::OK)
      .header(header::CONTENT_TYPE, content_type)
      .header(header::CONTENT_LENGTH, file_size.to_string())
      .header(header::CONTENT_DISPOSITION, content_disposition)
      .body(body)
      .map_err(|_| SumidaError::InternalServerError("Failed to create response".into()))?;

    Ok(response)
  }
  async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError>;
  fn get_uri(&self, path: &str) -> String;
}

pub struct SumidaStorage {
  inner: StorageBackend,
}

enum StorageBackend {
  Local(LocalStorage),
  #[cfg(feature = "storage-aws-s3")]
  S3(S3Storage),
}

impl SumidaStorage {
  pub fn new() -> Self {
    let settings = settings::get();

    #[cfg(feature = "storage-aws-s3")]
    if let Some(s3_settings) = &settings.storage.s3 {
      return Self {
        inner: StorageBackend::S3(S3Storage::new(s3_settings)),
      };
    }

    if let Some(local_settings) = &settings.storage.local {
      return Self {
        inner: StorageBackend::Local(LocalStorage::new(local_settings)),
      };
    }

    let default_settings = settings::storage::LocalStorageSettings::default();
    Self {
      inner: StorageBackend::Local(LocalStorage::new(&default_settings)),
    }
  }
}

impl Default for SumidaStorage {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl Storage for SumidaStorage {
  async fn store(&self, file: StaticFile) -> Result<(), SumidaError> {
    match &self.inner {
      StorageBackend::Local(storage) => storage.store(&file).await,
      #[cfg(feature = "storage-aws-s3")]
      StorageBackend::S3(storage) => storage.store(&file).await,
    }
  }

  async fn delete(&self, path: &str) -> Result<(), SumidaError> {
    match &self.inner {
      StorageBackend::Local(storage) => storage.delete(path).await,
      #[cfg(feature = "storage-aws-s3")]
      StorageBackend::S3(storage) => storage.delete(path).await,
    }
  }

  async fn stream(&self, path: &str) -> Result<FileStream, SumidaError> {
    match &self.inner {
      StorageBackend::Local(storage) => storage.stream(path).await,
      #[cfg(feature = "storage-aws-s3")]
      StorageBackend::S3(storage) => storage.stream(path).await,
    }
  }

  async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    match &self.inner {
      StorageBackend::Local(storage) => storage.get_file_size(path).await,
      #[cfg(feature = "storage-aws-s3")]
      StorageBackend::S3(storage) => storage.get_file_size(path).await,
    }
  }

  fn get_uri(&self, path: &str) -> String {
    match &self.inner {
      StorageBackend::Local(storage) => storage.get_uri(path),
      #[cfg(feature = "storage-aws-s3")]
      StorageBackend::S3(storage) => storage.get_uri(path),
    }
  }
}

struct LocalStorage {
  base_directory: PathBuf,
}

impl LocalStorage {
  fn new(settings: &settings::storage::LocalStorageSettings) -> Self {
    Self {
      base_directory: PathBuf::from(&settings.directory),
    }
  }

  /// Ensures the path resolves only within `base_directory`.
  /// If a `..` (parent directory) or absolute-path component comes in, `PathBuf::join` escapes outside base,
  /// so allow only normal components and reject anything else.
  fn resolve_path(&self, relative: &str) -> Result<PathBuf, SumidaError> {
    use std::path::{Component, Path};

    let rel = Path::new(relative);
    for component in rel.components() {
      match component {
        Component::Normal(_) | Component::CurDir => {}
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
          return Err(SumidaError::BadRequest(format!("Invalid file path: {relative}")));
        }
      }
    }

    Ok(self.base_directory.join(rel))
  }

  async fn store(&self, file: &StaticFile) -> Result<(), SumidaError> {
    use tokio::fs;

    let target_path = self.resolve_path(&file.get_path())?;

    if let Some(parent) = target_path.parent() {
      fs::create_dir_all(parent).await.map_err(|err| {
        SumidaError::InternalServerError(format!("Failed to create upload directory: {err}"))
      })?;
    }

    fs::write(&target_path, &file.bytes)
      .await
      .map_err(|err| SumidaError::InternalServerError(format!("Failed to write file: {err}")))?;

    Ok(())
  }

  async fn delete(&self, path: &str) -> Result<(), SumidaError> {
    use tokio::fs;

    let target_path = self.resolve_path(path)?;
    fs::remove_file(&target_path)
      .await
      .map_err(|err| SumidaError::InternalServerError(format!("Failed to delete file: {err}")))?;

    Ok(())
  }

  async fn stream(&self, path: &str) -> Result<FileStream, SumidaError> {
    use tokio::fs::File;

    let file = File::open(self.resolve_path(path)?)
      .await
      .map_err(|_| SumidaError::NoData("File".into(), path.to_string()))?;

    Ok(Box::pin(file))
  }

  async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    use tokio::fs;

    let metadata = fs::metadata(self.resolve_path(path)?)
      .await
      .map_err(|_| SumidaError::NoData("File".into(), path.to_string()))?;

    Ok(metadata.len())
  }

  fn get_uri(&self, path: &str) -> String {
    // Returns the resolved absolute path for a normal path, or an empty string for an escaping path.
    self.resolve_path(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
  }
}

#[cfg(feature = "storage-aws-s3")]
struct S3Storage {
  bucket: String,
  prefix: Option<String>,
  client: aws_sdk_s3::Client,
}

#[cfg(feature = "storage-aws-s3")]
impl S3Storage {
  fn new(settings: &settings::storage::S3StorageSettings) -> Self {
    let client = s3::new_client();

    Self {
      bucket: settings.bucket.clone(),
      prefix: settings.prefix.clone(),
      client,
    }
  }

  fn get_key(&self, path: &str) -> String {
    let stage = &settings::get().server.stage;

    match &self.prefix {
      Some(prefix) => format!("static/{stage}/{prefix}/{path}"),
      None => path.to_string(),
    }
  }

  async fn store(&self, file: &StaticFile) -> Result<(), SumidaError> {
    use aws_sdk_s3::primitives::ByteStream;

    let key = self.get_key(&file.get_path());

    self
      .client
      .put_object()
      .bucket(&self.bucket)
      .key(&key)
      .content_type(file.mime_type.clone())
      .body(ByteStream::from(file.bytes.clone()))
      .send()
      .await
      .map_err(|err| SumidaError::InternalServerError(format!("Failed to upload to S3: {err}")))?;

    Ok(())
  }

  async fn delete(&self, path: &str) -> Result<(), SumidaError> {
    let key = self.get_key(path);

    self
      .client
      .delete_object()
      .bucket(&self.bucket)
      .key(key)
      .send()
      .await
      .map_err(|err| SumidaError::InternalServerError(format!("Failed to delete from S3: {err}")))?;

    Ok(())
  }

  async fn stream(&self, path: &str) -> Result<FileStream, SumidaError> {
    use std::io::Cursor;

    let response = self.client.get_object().bucket(&self.bucket).key(self.get_key(path)).send().await?;

    // Collect the entire ByteStream into memory
    let bytes = response
      .body
      .collect()
      .await
      .map_err(|err| SumidaError::InternalServerError(format!("Failed to read S3 object: {err}")))?
      .into_bytes();

    // Create a Cursor over the bytes, which implements AsyncRead
    let cursor = Cursor::new(bytes);

    Ok(Box::pin(cursor))
  }

  async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    let key = self.get_key(path);
    let response = self.client.head_object().bucket(&self.bucket).key(key).send().await?;

    Ok(response.content_length().unwrap_or(0) as u64)
  }

  fn get_uri(&self, path: &str) -> String {
    format!("https://{}.s3.amazonaws.com/{}", self.bucket, self.get_key(path))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // percent_encode_rfc5987 (M-8): Content-Disposition filename* encoding
  #[test]
  fn rfc5987_keeps_attr_chars() {
    assert_eq!(percent_encode_rfc5987("plain_file-1.0.txt"), "plain_file-1.0.txt");
  }

  #[test]
  fn rfc5987_encodes_space_and_quote() {
    assert_eq!(percent_encode_rfc5987("a b"), "a%20b");
    assert_eq!(percent_encode_rfc5987("a\"b"), "a%22b");
  }

  #[test]
  fn rfc5987_encodes_utf8() {
    assert_eq!(percent_encode_rfc5987("é"), "%C3%A9");
  }
}
