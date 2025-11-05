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

    let response = Response::builder()
      .status(StatusCode::OK)
      .header(header::CONTENT_TYPE, content_type)
      .header(header::CONTENT_LENGTH, file_size.to_string())
      .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file_name}\""))
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

  fn resolve_path(&self, relative: &str) -> PathBuf {
    self.base_directory.join(relative)
  }

  async fn store(&self, file: &StaticFile) -> Result<(), SumidaError> {
    use tokio::fs;

    let target_path = self.resolve_path(&file.get_path());

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

    let target_path = self.resolve_path(path);
    fs::remove_file(&target_path)
      .await
      .map_err(|err| SumidaError::InternalServerError(format!("Failed to delete file: {err}")))?;

    Ok(())
  }

  async fn stream(&self, path: &str) -> Result<FileStream, SumidaError> {
    use tokio::fs::File;

    let file = File::open(self.resolve_path(path))
      .await
      .map_err(|_| SumidaError::NoData("File".into(), path.to_string()))?;

    Ok(Box::pin(file))
  }

  async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    use tokio::fs;

    let metadata = fs::metadata(self.resolve_path(path))
      .await
      .map_err(|_| SumidaError::NoData("File".into(), path.to_string()))?;

    Ok(metadata.len())
  }

  fn get_uri(&self, path: &str) -> String {
    self.resolve_path(path).to_string_lossy().into_owned()
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
