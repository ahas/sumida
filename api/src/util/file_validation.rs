use entity::bs::boards;
use error::SumidaError;
use sea_orm::{ConnectionTrait, EntityTrait};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FileValidationConfig {
  pub can_upload_file: bool,
  pub max_file_size: i64,
  pub max_file_count: i32,
  pub allowed_file_types: Vec<String>,
}

impl From<&boards::Model> for FileValidationConfig {
  fn from(board: &boards::Model) -> Self {
    Self {
      can_upload_file: board.can_upload_file,
      max_file_size: board.max_file_size,
      max_file_count: board.max_file_count,
      allowed_file_types: board.get_allowed_file_types(),
    }
  }
}

#[derive(Debug, Clone)]
pub struct FileInfo {
  pub filename: String,
  pub size: i64,
  pub mime_type: String,
  pub extension: Option<String>,
}

impl FileInfo {
  pub fn new(filename: String, size: i64, mime_type: String) -> Self {
    let extension =
      Path::new(&filename).extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_lowercase());

    Self {
      filename,
      size,
      mime_type,
      extension,
    }
  }

  pub fn from_multipart_field(filename: String, size: i64, content_type: Option<String>) -> Self {
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

#[derive(Debug)]
pub enum FileValidationError {
  UploadDisabled,
  FileTooLarge {
    size: i64,
    max_size: i64,
  },
  InvalidFileType {
    extension: String,
    allowed_types: Vec<String>,
  },
  TooManyFiles {
    current_count: i32,
    max_count: i32,
  },
  InvalidFilename(String),
  UnsupportedMimeType(String),
}

impl From<FileValidationError> for SumidaError {
  fn from(err: FileValidationError) -> Self {
    match err {
      FileValidationError::UploadDisabled => {
        SumidaError::BadRequest("File upload is disabled for this board".into())
      }
      FileValidationError::FileTooLarge { size, max_size } => SumidaError::BadRequest(format!(
        "File size {} bytes exceeds maximum allowed size {} bytes",
        size, max_size
      )),
      FileValidationError::InvalidFileType {
        extension,
        allowed_types,
      } => SumidaError::BadRequest(format!(
        "File type '{}' is not allowed. Allowed types: {}",
        extension,
        allowed_types.join(", ")
      )),
      FileValidationError::TooManyFiles {
        current_count,
        max_count,
      } => SumidaError::BadRequest(format!(
        "Cannot upload more files. Current: {}, Maximum: {}",
        current_count, max_count
      )),
      FileValidationError::InvalidFilename(filename) => {
        SumidaError::BadRequest(format!("Invalid filename: {}", filename))
      }
      FileValidationError::UnsupportedMimeType(mime_type) => {
        SumidaError::BadRequest(format!("Unsupported MIME type: {}", mime_type))
      }
    }
  }
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

  pub fn validate_upload_enabled(&self) -> Result<(), FileValidationError> {
    if !self.config.can_upload_file {
      return Err(FileValidationError::UploadDisabled);
    }
    Ok(())
  }

  pub fn validate_file_size(&self, file: &FileInfo) -> Result<(), FileValidationError> {
    if self.config.max_file_size != 0 && file.size > self.config.max_file_size {
      return Err(FileValidationError::FileTooLarge {
        size: file.size,
        max_size: self.config.max_file_size,
      });
    }

    Ok(())
  }

  pub fn validate_file_type(&self, file: &FileInfo) -> Result<(), FileValidationError> {
    if let Some(extension) = &file.extension {
      if self.config.allowed_file_types.len() > 0
        && !self.config.allowed_file_types.iter().any(|t| t.to_lowercase() == extension.to_lowercase())
      {
        return Err(FileValidationError::InvalidFileType {
          extension: extension.clone(),
          allowed_types: self.config.allowed_file_types.clone(),
        });
      }
    } else {
      return Err(FileValidationError::InvalidFilename(file.filename.clone()));
    }

    Ok(())
  }

  pub fn validate_file_count(&self, current_count: i32) -> Result<(), FileValidationError> {
    if self.config.max_file_count != 0 && current_count >= self.config.max_file_count {
      return Err(FileValidationError::TooManyFiles {
        current_count,
        max_count: self.config.max_file_count,
      });
    }

    Ok(())
  }

  pub fn validate_filename(&self, filename: &str) -> Result<(), FileValidationError> {
    // Basic filename validation
    if filename.is_empty() || filename.len() > 255 {
      return Err(FileValidationError::InvalidFilename(filename.to_string()));
    }

    // Check for dangerous characters
    if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
      return Err(FileValidationError::InvalidFilename(filename.to_string()));
    }

    // Check for reserved names (Windows)
    let name_upper = filename.to_uppercase();
    if [
      "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
      "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .iter()
    .any(|&reserved| name_upper.starts_with(reserved))
    {
      return Err(FileValidationError::InvalidFilename(filename.to_string()));
    }

    Ok(())
  }

  pub fn validate_mime_type(&self, file: &FileInfo) -> Result<(), FileValidationError> {
    // Basic MIME type validation - ensure it's not empty and follows basic format
    if file.mime_type.is_empty() || !file.mime_type.contains("/") {
      return Err(FileValidationError::UnsupportedMimeType(file.mime_type.clone()));
    }

    // Additional security checks for dangerous MIME types
    let dangerous_types = [
      "application/x-executable",
      "application/x-msdownload",
      "application/x-msdos-program",
      "application/x-sh",
      "text/x-script",
    ];

    if dangerous_types.iter().any(|&dangerous| file.mime_type == dangerous) {
      return Err(FileValidationError::UnsupportedMimeType(file.mime_type.clone()));
    }

    Ok(())
  }

  pub fn validate_file(&self, file: &FileInfo) -> Result<(), FileValidationError> {
    self.validate_upload_enabled()?;
    self.validate_filename(&file.filename)?;
    self.validate_file_size(file)?;
    self.validate_file_type(file)?;
    self.validate_mime_type(file)?;
    Ok(())
  }

  pub fn validate_file_with_count(
    &self,
    file: &FileInfo,
    current_count: i32,
  ) -> Result<(), FileValidationError> {
    self.validate_file_count(current_count)?;
    self.validate_file(file)?;
    Ok(())
  }

  pub fn get_config(&self) -> &FileValidationConfig {
    &self.config
  }
}

/// Detect MIME type from file extension as fallback
fn detect_mime_type_from_extension(filename: &str) -> String {
  let extension = Path::new(filename).extension().and_then(|ext| ext.to_str()).unwrap_or("").to_lowercase();

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
