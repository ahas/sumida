#[cfg(feature = "aws-s3")]
use aws_sdk_s3::error::SdkError as S3Error;
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::DbErr;
use serde_json::json;
use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum SumidaError {
  // do not use this error type in general situations
  #[error("status: {0}, reason: {1}")]
  Unknown(u16, String),

  // data errors
  #[error("type: {0}, key: {1}")]
  NoData(String, String),
  #[error("no content to serve")]
  NoContent,

  // authorization errors
  #[error("invalid account")]
  InvalidAccount,
  #[error("inactive account")]
  InactiveAccount,
  #[error("account is banned")]
  AccountBanned,
  #[error("user organization id '{0}' not found")]
  NoUserOrganization(i32),
  #[error("user role id `{0}` not found")]
  NoUserRole(i32),
  #[error("user id `{0}` have no email")]
  NoUserEmail(i32),
  #[error("user id `{0}` not authorized")]
  NotAuthorized(i32),
  #[error("user credentials required")]
  NoCredentials,
  #[error("no access permission for this resource")]
  NoAccessPermission,
  #[error("no service available")]
  NoServiceAvailable,
  #[error("no jwt secret")]
  NoJwtSecret,
  #[error("invalid jwt")]
  InvalidJwt,
  #[error("jwt generation failed")]
  JwtGenerationFailed,
  #[error("invalid jwt expiry")]
  InvalidJwtExpiry,
  #[error("invalid api key")]
  InvalidApiKey,
  #[error("invalid authorization token")]
  InvalidAuthorizationToken,
  #[error("invalid authorization method `{0}`")]
  InvalidAuthorizationMethod(String),
  #[error("accessible to administrators only")]
  OnlyAdminAllowed,
  #[error("invalid user agent")]
  InvalidUserAgent,

  #[error("no mail form `{0}`")]
  NoMailForm(String),
  #[error("could not add mail form template: reason: `{0}`")]
  MailFormAddTemplateFailed(String),
  #[error("could not render mail form: reason: `{0}`")]
  MailFormRenderFailed(String),
  #[error("could not parse email address")]
  EmailParseFailed,
  #[error("could not build a e-mail message: reason: `{0}`")]
  EmailMessageBuildFailed(String),
  #[error("could not send email: reason: `{0}`")]
  EmailSendFailed(String),
  #[error("invalid verification code")]
  VerificationFailed,

  #[error("file upload is not allowed")]
  FileUploadNotAllowed,
  #[error("cannot upload more files. current: {current_count}, maximum: {max_count}")]
  TooManyFiles {
    current_count: usize,
    max_count:     usize,
  },
  #[error("file type '{extension}' is not allowed. allowed types: {}", allowed_types.join(", "))]
  InvalidFileType {
    extension:     String,
    allowed_types: Vec<String>,
  },
  #[error("invalid filename: {0}")]
  InvalidFilename(String),
  #[error("unsupported Mime type: {0}")]
  UnsupportedMimeType(String),
  #[error("file size {size} bytes exceeds maximum allowed size {max_size} bytes")]
  FileTooLarge { size: usize, max_size: usize },
  #[error("file does not exist")]
  FileNotFound,
  #[error("reusing uploaded files is not permitted")]
  FileReuseNotAllowed,

  // runtime errors
  #[error("reason: {0}")]
  Unsupported(String),
  #[error("reason: {0}")]
  Timeout(String),
  #[error("reason: {0}")]
  Busy(String),
  #[error("client disconnected")]
  Disconnected,
  #[error("internal server error: {0}")]
  InternalServerError(String),

  // validation errors
  #[error("bad request: {0}")]
  BadRequest(String),

  // external crate errors
  #[error("status: {0}, reason: {1}")]
  DatabaseError(u16, String),
  #[cfg(feature = "aws-s3")]
  #[error("status: {0}, reason: {1}")]
  AwsS3SdkError(u16, String),
}

impl SumidaError {
  fn code(&self) -> String {
    let n: i32 = match self {
      Self::NoContent => 0,
      _ => -1,
    };

    format!("E{n:04}")
  }
}

// impl Into<StatusCode> for SumidaError {
impl From<SumidaError> for StatusCode {
  fn from(err: SumidaError) -> Self {
    match err {
      SumidaError::Unknown(v, _) => StatusCode::from_u16(v),

      // data errors
      SumidaError::NoData(_, _) => Ok(StatusCode::NOT_FOUND),
      SumidaError::NoContent => Ok(StatusCode::NO_CONTENT),

      // authorization errors
      SumidaError::InvalidAccount
      | SumidaError::InactiveAccount
      | SumidaError::AccountBanned
      | SumidaError::NotAuthorized(_)
      | SumidaError::NoCredentials
      | SumidaError::NoUserOrganization(_)
      | SumidaError::NoUserRole(_)
      | SumidaError::InvalidJwt
      | SumidaError::JwtGenerationFailed
      | SumidaError::InvalidJwtExpiry
      | SumidaError::NoJwtSecret
      | SumidaError::InvalidApiKey
      | SumidaError::InvalidAuthorizationToken
      | SumidaError::OnlyAdminAllowed
      | SumidaError::VerificationFailed => Ok(StatusCode::UNAUTHORIZED),
      SumidaError::NoAccessPermission => Ok(StatusCode::FORBIDDEN),
      SumidaError::NoServiceAvailable => Ok(StatusCode::FORBIDDEN),
      SumidaError::InvalidAuthorizationMethod(_) => Ok(StatusCode::NOT_IMPLEMENTED),

      // runtime errors
      SumidaError::Unsupported(_) => Ok(StatusCode::NOT_IMPLEMENTED),
      SumidaError::Timeout(_) => Ok(StatusCode::REQUEST_TIMEOUT),
      SumidaError::Busy(_) => Ok(StatusCode::TOO_MANY_REQUESTS),

      // validation errors
      SumidaError::BadRequest(_) => Ok(StatusCode::BAD_REQUEST),

      // external crate errors
      SumidaError::DatabaseError(v, _) => StatusCode::from_u16(v),
      #[cfg(feature = "aws-s3")]
      SumidaError::AwsS3SdkError(v, _) => StatusCode::from_u16(v),

      _ => Ok(StatusCode::INTERNAL_SERVER_ERROR),
    }
    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
  }
}

impl IntoResponse for SumidaError {
  fn into_response(self) -> axum::response::Response {
    let code = self.code();
    let status_code: StatusCode = self.clone().into();
    let reason = self.to_string();
    tracing::error!("SumidaError: {}, {}", status_code, reason);

    (
      status_code,
      if cfg!(debug_assertions) {
        Json(json!({
          "code": code,
          "statusCode": status_code.as_u16(),
          "reason": reason,
        }))
      } else {
        Json(json!({
          "code": code,
          "statusCode": status_code.as_u16(),
        }))
      },
    )
      .into_response()
  }
}

impl From<DbErr> for SumidaError {
  fn from(value: DbErr) -> Self {
    let (status_code, reason) = match value {
      DbErr::RecordNotFound(_) | DbErr::RecordNotUpdated => (StatusCode::NOT_FOUND, value.to_string()),
      DbErr::RecordNotInserted => (StatusCode::CONFLICT, value.to_string()),
      _ => (StatusCode::INTERNAL_SERVER_ERROR, value.to_string()),
    };

    SumidaError::DatabaseError(status_code.as_u16(), reason)
  }
}

#[cfg(feature = "aws-s3")]
impl<T> From<S3Error<T>> for SumidaError {
  fn from(err: S3Error<T>) -> Self {
    let status_code = if let Some(resp) = err.raw_response() {
      StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    } else {
      StatusCode::INTERNAL_SERVER_ERROR
    };

    SumidaError::AwsS3SdkError(status_code.as_u16(), err.to_string())
  }
}

impl From<anyhow::Error> for SumidaError {
  fn from(value: anyhow::Error) -> Self {
    tracing::error!("Unknown: {:?}", value.backtrace());
    SumidaError::Unknown(500, value.to_string())
  }
}
