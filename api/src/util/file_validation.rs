use std::path::Path;

use entity::bps::boards;
use error::SumidaError;
use sea_orm::{ConnectionTrait, EntityTrait};

#[derive(Debug, Clone)]
pub struct FileValidationConfig {
  pub can_upload_file: bool,
  pub max_file_size: usize,
  pub max_file_count: usize,
  pub allowed_file_types: Vec<String>,
}

impl From<&boards::Model> for FileValidationConfig {
  fn from(board: &boards::Model) -> Self {
    Self {
      can_upload_file: board.can_upload_file,
      max_file_size: board.max_file_size as usize,
      max_file_count: board.max_file_count as usize,
      allowed_file_types: board.get_allowed_file_types(),
    }
  }
}

#[derive(Debug, Clone)]
pub struct FileInfo {
  pub size: usize,
  pub mime_type: String,
  pub extension: Option<String>,
}

impl FileInfo {
  pub fn new(filename: String, size: usize, mime_type: String) -> Self {
    let extension = Path::new(&filename)
      .extension()
      .and_then(|ext| ext.to_str())
      .map(|ext| ext.to_lowercase());

    Self {
      size,
      mime_type,
      extension,
    }
  }

  pub fn from_multipart_field(filename: String, size: usize, content_type: Option<String>) -> Self {
    let mime_type = content_type.unwrap_or_else(|| {
      // Fallback MIME type detection based on file extension
      detect_mime_type_from_extension(&filename)
    });

    Self::new(filename, size, mime_type)
  }

  pub fn get_extension(&self) -> Option<&str> {
    self.extension.as_deref()
  }

  pub fn is_image(&self) -> bool {
    self.mime_type.starts_with("image/")
  }

  pub fn is_video(&self) -> bool {
    self.mime_type.starts_with("video/")
  }

  pub fn is_audio(&self) -> bool {
    self.mime_type.starts_with("audio/")
  }

  pub fn is_document(&self) -> bool {
    matches!(
      self.mime_type.as_str(),
      "application/pdf"
        | "application/msword"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.ms-excel"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "text/plain"
        | "text/csv"
    )
  }
}

pub trait IntoFileInfo {
  fn into_file_info(&self) -> FileInfo;
}

pub struct FileValidator {
  config: FileValidationConfig,
}

impl FileValidator {
  pub fn new(config: FileValidationConfig) -> Self {
    Self { config }
  }

  pub async fn from_board_id<C>(board_id: i32, db: &C) -> Result<Self, SumidaError>
  where
    C: ConnectionTrait,
  {
    let board = boards::Entity::find_by_id(board_id)
      .one(db)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".into(), board_id.to_string()))?;

    Ok(Self::new(FileValidationConfig::from(&board)))
  }

  pub fn validate_upload_enabled(self) -> Result<Self, SumidaError> {
    if !self.config.can_upload_file {
      return Err(SumidaError::FileUploadNotAllowed);
    }

    Ok(self)
  }

  pub fn validate_size(self, size: usize) -> Result<Self, SumidaError> {
    if self.config.max_file_size != 0 && size > self.config.max_file_size {
      return Err(SumidaError::FileTooLarge {
        size,
        max_size: self.config.max_file_size,
      });
    }

    Ok(self)
  }

  pub fn validate_extension(self, extension: Option<&str>) -> Result<Self, SumidaError> {
    if let Some(extension) = extension {
      if !self.config.allowed_file_types.is_empty()
        && !self
          .config
          .allowed_file_types
          .iter()
          .any(|t| t.to_lowercase() == extension.to_lowercase())
      {
        return Err(SumidaError::InvalidFileType {
          extension: extension.to_string(),
          allowed_types: self.config.allowed_file_types.clone(),
        });
      }
    }

    Ok(self)
  }

  pub fn validate_count(self, current_count: usize) -> Result<Self, SumidaError> {
    if self.config.max_file_count != 0 && current_count >= self.config.max_file_count {
      return Err(SumidaError::TooManyFiles {
        current_count,
        max_count: self.config.max_file_count,
      });
    }

    Ok(self)
  }

  pub fn validate_mime_type(self, mime_type: &str) -> Result<Self, SumidaError> {
    // Basic MIME type validation - ensure it's not empty and follows basic format
    if mime_type.is_empty() || !mime_type.contains("/") {
      return Err(SumidaError::UnsupportedMimeType(mime_type.to_string()));
    }

    // Additional security checks for dangerous MIME types
    let dangerous_types = [
      "application/x-executable",
      "application/x-msdownload",
      "application/x-msdos-program",
      "application/x-sh",
      "text/x-script",
    ];

    if dangerous_types.iter().any(|&dangerous| mime_type == dangerous) {
      return Err(SumidaError::UnsupportedMimeType(mime_type.to_string()));
    }

    Ok(self)
  }

  pub fn validate_file(self, file: &FileInfo) -> Result<Self, SumidaError> {
    self
      .validate_upload_enabled()?
      .validate_size(file.size)?
      .validate_extension(file.get_extension())?
      .validate_mime_type(&file.mime_type)
  }

  pub fn validate_file_with_count(
    self,
    file: &FileInfo,
    current_count: usize,
  ) -> Result<Self, SumidaError> {
    self.validate_count(current_count)?.validate_file(file)
  }

  pub fn get_config(&self) -> &FileValidationConfig {
    &self.config
  }
}

/// Detect MIME type from file extension as fallback
fn detect_mime_type_from_extension(filename: &str) -> String {
  let extension = Path::new(filename)
    .extension()
    .and_then(|ext| ext.to_str())
    .unwrap_or("")
    .to_lowercase();

  match extension.as_str() {
    // Images
    "jpg" | "jpeg" => "image/jpeg",
    "png" => "image/png",
    "gif" => "image/gif",
    "webp" => "image/webp",
    "svg" => "image/svg+xml",
    "bmp" => "image/bmp",
    "ico" => "image/x-icon",

    // Documents
    "pdf" => "application/pdf",
    "doc" => "application/msword",
    "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "xls" => "application/vnd.ms-excel",
    "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "ppt" => "application/vnd.ms-powerpoint",
    "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",

    // Text
    "txt" => "text/plain",
    "csv" => "text/csv",
    "html" | "htm" => "text/html",
    "css" => "text/css",
    "js" => "application/javascript",
    "json" => "application/json",
    "xml" => "application/xml",

    // Archives
    "zip" => "application/zip",
    "rar" => "application/vnd.rar",
    "7z" => "application/x-7z-compressed",
    "tar" => "application/x-tar",
    "gz" => "application/gzip",

    // Audio
    "mp3" => "audio/mpeg",
    "wav" => "audio/wav",
    "ogg" => "audio/ogg",
    "m4a" => "audio/mp4",

    // Video
    "mp4" => "video/mp4",
    "avi" => "video/x-msvideo",
    "mov" => "video/quicktime",
    "wmv" => "video/x-ms-wmv",
    "flv" => "video/x-flv",
    "webm" => "video/webm",

    // Default
    _ => "application/octet-stream",
  }
  .to_string()
}
