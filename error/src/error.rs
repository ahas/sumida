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
    max_count: usize,
  },
  #[error("file type '{extension}' is not allowed. allowed types: {}", allowed_types.join(", "))]
  InvalidFileType {
    extension: String,
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
  #[error("\"{0}\" field not provided")]
  FieldNotProvided(String),

  // membership / billing errors
  #[error("membership plan `{0}` not found")]
  PlanNotFound(String),
  #[error("no active subscription for this action")]
  NoActiveSubscription,
  #[error("subscription has expired")]
  SubscriptionExpired,
  #[error("action `{0}` is not allowed by the current membership")]
  MembershipActionNotAllowed(String),
  #[error("plan scope does not match the subscription owner")]
  MembershipPlanScopeMismatch,
  #[error("an active subscription for this plan already exists")]
  DuplicateActiveSubscription,
  #[error("{0}")]
  Conflict(String),
  #[error("insufficient credits: required {required}, available {available}")]
  InsufficientCredits { required: i64, available: i64 },
  #[error("refund amount {0} exceeds available balance {1}")]
  RefundWouldExceedBalance(i64, i64),
  #[error("credit wallet balance diverges from ledger (wallet `{0}`)")]
  BalanceDivergence(i32),
  #[error("purchase `{0}` not found")]
  PurchaseNotFound(String),
  #[error("payment is not configured")]
  PaymentNotConfigured,
  #[error("payment provider error: {0}")]
  PaymentProviderError(String),
  #[error("payment not completed (status: {0})")]
  PaymentNotCompleted(String),
  #[error("payment amount mismatch: expected {expected}, paid {actual}")]
  PaymentAmountMismatch { expected: i64, actual: i64 },
  #[error("webhook verification failed")]
  WebhookVerificationFailed,

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
  /// A stable error code per variant. Release builds hide `reason`, so clients
  /// distinguish error kinds by this code. Categories: 0 data-no-content, 1xxx auth/authz,
  /// 2xxx data, 3xxx mail/verification, 4xxx file, 5xxx runtime, 6xxx request, 7xxx external, 9xxx uncategorized.
  fn code(&self) -> String {
    let n: i32 = match self {
      Self::NoContent => 0,

      Self::InvalidAccount => 1000,
      Self::InactiveAccount => 1001,
      Self::AccountBanned => 1002,
      Self::NoUserOrganization(_) => 1003,
      Self::NoUserRole(_) => 1004,
      Self::NoUserEmail(_) => 1005,
      Self::NotAuthorized(_) => 1006,
      Self::NoCredentials => 1007,
      Self::NoAccessPermission => 1008,
      Self::NoServiceAvailable => 1009,
      Self::NoJwtSecret => 1010,
      Self::InvalidJwt => 1011,
      Self::JwtGenerationFailed => 1012,
      Self::InvalidJwtExpiry => 1013,
      Self::InvalidApiKey => 1014,
      Self::InvalidAuthorizationToken => 1015,
      Self::InvalidAuthorizationMethod(_) => 1016,
      Self::OnlyAdminAllowed => 1017,
      Self::InvalidUserAgent => 1018,

      Self::NoData(..) => 2000,

      Self::NoMailForm(_) => 3000,
      Self::MailFormAddTemplateFailed(_) => 3001,
      Self::MailFormRenderFailed(_) => 3002,
      Self::EmailParseFailed => 3003,
      Self::EmailMessageBuildFailed(_) => 3004,
      Self::EmailSendFailed(_) => 3005,
      Self::VerificationFailed => 3006,

      Self::FileUploadNotAllowed => 4000,
      Self::TooManyFiles { .. } => 4001,
      Self::InvalidFileType { .. } => 4002,
      Self::InvalidFilename(_) => 4003,
      Self::UnsupportedMimeType(_) => 4004,
      Self::FileTooLarge { .. } => 4005,
      Self::FileNotFound => 4006,
      Self::FileReuseNotAllowed => 4007,
      Self::FieldNotProvided(_) => 4008,

      Self::Unsupported(_) => 5000,
      Self::Timeout(_) => 5001,
      Self::Busy(_) => 5002,
      Self::Disconnected => 5003,
      Self::InternalServerError(_) => 5004,

      Self::BadRequest(_) => 6000,

      Self::PlanNotFound(_) => 8000,
      Self::NoActiveSubscription => 8001,
      Self::SubscriptionExpired => 8002,
      Self::MembershipActionNotAllowed(_) => 8003,
      Self::MembershipPlanScopeMismatch => 8004,
      Self::DuplicateActiveSubscription => 8005,
      Self::Conflict(_) => 8009,
      Self::InsufficientCredits { .. } => 8006,
      Self::RefundWouldExceedBalance(..) => 8007,
      Self::BalanceDivergence(_) => 8008,
      Self::PurchaseNotFound(_) => 8009,
      Self::PaymentNotConfigured => 8010,
      Self::PaymentProviderError(_) => 8011,
      Self::PaymentNotCompleted(_) => 8012,
      Self::PaymentAmountMismatch { .. } => 8013,
      Self::WebhookVerificationFailed => 8014,

      Self::DatabaseError(..) => 7000,
      #[cfg(feature = "aws-s3")]
      Self::AwsS3SdkError(..) => 7001,

      Self::Unknown(..) => 9000,
    };

    format!("E{n:04}")
  }
}

/// Trait that makes downstream domain errors follow sumida's standard response schema.
///
/// A downstream service implements this trait on its own enum and calls
/// `SumidaApiError::to_response(&self)` in `into_response`, producing the same
/// JSON format (`{ code, statusCode, reason? }`) as sumida's `SumidaError`.
///
/// ```ignore
/// impl SumidaApiError for DaqsError {
///   fn status_code(&self) -> StatusCode { ... }
///   fn error_code(&self) -> String { "lab-app.something".into() }
/// }
///
/// impl IntoResponse for DaqsError {
///   fn into_response(self) -> Response { self.to_response() }
/// }
/// ```
pub trait SumidaApiError: std::error::Error {
  fn status_code(&self) -> StatusCode;

  fn error_code(&self) -> String {
    "E-1".to_owned()
  }

  fn to_response(&self) -> axum::response::Response {
    let status_code = self.status_code();
    let code = self.error_code();
    let reason = self.to_string();
    tracing::error!("ApiError: {}, {}", status_code, reason);

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

impl SumidaApiError for SumidaError {
  fn status_code(&self) -> StatusCode {
    self.clone().into()
  }

  fn error_code(&self) -> String {
    SumidaError::code(self)
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

      // membership / billing errors
      SumidaError::PlanNotFound(_) => Ok(StatusCode::NOT_FOUND),
      SumidaError::NoActiveSubscription
      | SumidaError::SubscriptionExpired
      | SumidaError::MembershipActionNotAllowed(_) => Ok(StatusCode::FORBIDDEN),
      SumidaError::InsufficientCredits { .. } => Ok(StatusCode::PAYMENT_REQUIRED),
      SumidaError::MembershipPlanScopeMismatch | SumidaError::RefundWouldExceedBalance(..) => {
        Ok(StatusCode::BAD_REQUEST)
      }
      SumidaError::DuplicateActiveSubscription => Ok(StatusCode::CONFLICT),
      SumidaError::Conflict(_) => Ok(StatusCode::CONFLICT),
      SumidaError::PurchaseNotFound(_) => Ok(StatusCode::NOT_FOUND),
      SumidaError::PaymentNotConfigured => Ok(StatusCode::SERVICE_UNAVAILABLE),
      SumidaError::PaymentProviderError(_) => Ok(StatusCode::BAD_GATEWAY),
      SumidaError::PaymentNotCompleted(_) => Ok(StatusCode::PAYMENT_REQUIRED),
      SumidaError::PaymentAmountMismatch { .. } => Ok(StatusCode::BAD_REQUEST),
      SumidaError::WebhookVerificationFailed => Ok(StatusCode::BAD_REQUEST),

      // runtime errors
      SumidaError::Unsupported(_) => Ok(StatusCode::NOT_IMPLEMENTED),
      SumidaError::Timeout(_) => Ok(StatusCode::REQUEST_TIMEOUT),
      SumidaError::Busy(_) => Ok(StatusCode::TOO_MANY_REQUESTS),

      // validation errors
      SumidaError::BadRequest(_) => Ok(StatusCode::BAD_REQUEST),

      // file upload validation (client errors, not 500)
      SumidaError::FileUploadNotAllowed
      | SumidaError::FileReuseNotAllowed
      | SumidaError::TooManyFiles { .. }
      | SumidaError::FieldNotProvided(_) => Ok(StatusCode::BAD_REQUEST),
      SumidaError::InvalidFileType { .. } | SumidaError::UnsupportedMimeType(_) => {
        Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE)
      }
      SumidaError::FileTooLarge { .. } => Ok(StatusCode::PAYLOAD_TOO_LARGE),

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
