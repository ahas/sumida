use std::path::Path;
use std::pin::Pin;

use error::SumidaError;
use infra::settings;
use tokio::io::AsyncRead;

#[cfg(feature = "storage-aws-s3")]
use crate::util::s3;

pub enum FileStorage {
  Local(LocalStorage),
  #[cfg(feature = "storage-aws-s3")]
  S3(S3Storage),
}

impl FileStorage {
  pub async fn store(&self, data: &[u8], filename: &str) -> Result<String, SumidaError> {
    match self {
      FileStorage::Local(storage) => storage.store(data, filename).await,
      #[cfg(feature = "storage-aws-s3")]
      FileStorage::S3(storage) => storage.store(data, filename).await,
    }
  }

  pub async fn delete(&self, path: &str) -> Result<(), SumidaError> {
    match self {
      FileStorage::Local(storage) => storage.delete(path).await,
      #[cfg(feature = "storage-aws-s3")]
      FileStorage::S3(storage) => storage.delete(path).await,
    }
  }

  pub async fn download(&self, path: &str) -> Result<Pin<Box<dyn AsyncRead + Send>>, SumidaError> {
    match self {
      FileStorage::Local(storage) => storage.download(path).await,
      #[cfg(feature = "storage-aws-s3")]
      FileStorage::S3(storage) => storage.download(path).await,
    }
  }

  pub async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    match self {
      FileStorage::Local(storage) => storage.get_file_size(path).await,
      #[cfg(feature = "storage-aws-s3")]
      FileStorage::S3(storage) => storage.get_file_size(path).await,
    }
  }

  pub fn get_url(&self, path: &str) -> String {
    match self {
      FileStorage::Local(storage) => storage.get_url(path),
      #[cfg(feature = "storage-aws-s3")]
      FileStorage::S3(storage) => storage.get_url(path),
    }
  }
}

pub fn create_storage() -> FileStorage {
  let settings = settings::get();

  #[cfg(feature = "storage-aws-s3")]
  if let Some(s3_settings) = &settings.storage.s3 {
    return FileStorage::S3(S3Storage::new(s3_settings));
  }

  if let Some(local_settings) = &settings.storage.local {
    return FileStorage::Local(LocalStorage::new(local_settings));
  }

  // Fallback to default local storage
  let default_settings = settings::storage::LocalStorageSettings::default();
  FileStorage::Local(LocalStorage::new(&default_settings))
}

pub struct LocalStorage {
  directory: String,
}

impl LocalStorage {
  pub fn new(settings: &settings::storage::LocalStorageSettings) -> Self {
    Self {
      directory: settings.directory.clone(),
    }
  }
}

impl LocalStorage {
  pub async fn store(&self, data: &[u8], filename: &str) -> Result<String, SumidaError> {
    use tokio::fs;
    use tokio::io::AsyncWriteExt;

    let file_path = format!("{}/{}", self.directory, filename);

    // Ensure upload directory exists
    fs::create_dir_all(&self.directory)
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to create upload directory: {}", e)))?;

    // Write file to disk
    let mut file = fs::File::create(&file_path)
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to create file: {}", e)))?;

    file
      .write_all(data)
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to write file: {}", e)))?;

    Ok(file_path)
  }

  pub async fn delete(&self, path: &str) -> Result<(), SumidaError> {
    use tokio::fs;

    fs::remove_file(path)
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to delete file: {}", e)))?;

    Ok(())
  }

  pub async fn download(&self, path: &str) -> Result<Pin<Box<dyn AsyncRead + Send>>, SumidaError> {
    use tokio::fs::File;

    let file = File::open(path)
      .await
      .map_err(|_| SumidaError::NoData("File".into(), path.to_string()))?;

    Ok(Box::pin(file))
  }

  pub async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    use tokio::fs;

    let metadata = fs::metadata(path)
      .await
      .map_err(|_| SumidaError::NoData("File".into(), path.to_string()))?;

    Ok(metadata.len())
  }

  pub fn get_url(&self, path: &str) -> String {
    // For local storage, return a relative API path for download
    let filename = Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or("unknown");

    format!("/api/board-files/local/{}", filename)
  }
}

#[cfg(feature = "storage-aws-s3")]
pub struct S3Storage {
  bucket: String,
  prefix: Option<String>,
  client: aws_sdk_s3::Client,
}

#[cfg(feature = "storage-aws-s3")]
impl S3Storage {
  pub fn new(settings: &settings::storage::S3StorageSettings) -> Self {
    let client = s3::new_client();

    Self {
      bucket: settings.bucket.clone(),
      prefix: settings.prefix.clone(),
      client,
    }
  }

  fn get_key(&self, filename: &str) -> String {
    let stage = &settings::get().server.stage;

    match &self.prefix {
      Some(prefix) => format!("static/{stage}/{}/{}", prefix, filename),
      None => filename.to_string(),
    }
  }
}

#[cfg(feature = "storage-aws-s3")]
impl S3Storage {
  pub async fn store(&self, data: &[u8], filename: &str) -> Result<String, SumidaError> {
    use aws_sdk_s3::primitives::ByteStream;

    let key = self.get_key(filename);

    self
      .client
      .put_object()
      .bucket(&self.bucket)
      .key(&key)
      .body(ByteStream::from(data.to_vec()))
      .send()
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to upload to S3: {}", e)))?;

    Ok(key)
  }

  pub async fn delete(&self, path: &str) -> Result<(), SumidaError> {
    self
      .client
      .delete_object()
      .bucket(&self.bucket)
      .key(path)
      .send()
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to delete from S3: {}", e)))?;

    Ok(())
  }

  pub async fn download(&self, path: &str) -> Result<Pin<Box<dyn AsyncRead + Send>>, SumidaError> {
    use std::io::Cursor;

    use aws_sdk_s3::primitives::ByteStream;

    let response = self
      .client
      .get_object()
      .bucket(&self.bucket)
      .key(path)
      .send()
      .await
      .map_err(|e| SumidaError::NoData("S3 Object".into(), format!("{}: {}", path, e)))?;

    // Collect the entire ByteStream into memory
    let bytes = response
      .body
      .collect()
      .await
      .map_err(|e| SumidaError::InternalServerError(format!("Failed to read S3 object: {}", e)))?
      .into_bytes();

    // Create a Cursor over the bytes, which implements AsyncRead
    let cursor = Cursor::new(bytes);
    Ok(Box::pin(cursor))
  }

  pub async fn get_file_size(&self, path: &str) -> Result<u64, SumidaError> {
    let response = self
      .client
      .head_object()
      .bucket(&self.bucket)
      .key(path)
      .send()
      .await
      .map_err(|e| SumidaError::NoData("S3 Object".into(), format!("{}: {}", path, e)))?;

    Ok(response.content_length().unwrap_or(0) as u64)
  }

  pub fn get_url(&self, path: &str) -> String {
    // Return S3 URL or presigned URL
    format!("https://{}.s3.amazonaws.com/{}", self.bucket, path)
  }
}
