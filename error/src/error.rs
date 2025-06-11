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
  #[error("status = {0}, reason = {1}")]
  Unknown(u16, String),

  // data errors
  #[error("type = {0}, key = {1}")]
  NoData(String, String),
  #[error("no content to serve")]
  NoContent,

  // authorization errors
  #[error("invalid account")]
  InvalidAccount,
  #[error("inactive account")]
  InactiveAccount,
  #[error("account blocked")]
  AccountBlocked,
  #[error("user organization id '{0}' not found")]
  NoUserOrganization(i32),
  #[error("user role id `{0}` not found")]
  NoUserRole(i32),
  #[error("user `{0}` have no email")]
  NoUserEmail(String),
  #[error("user `{0}` not authorized")]
  NotAuthorized(String),
  #[error("user credentials required")]
  NoCredentials,
  #[error("no permission to perform `{0}`")]
  NoAbilityTopic(String),
  #[error("permission `{0}` exists, but action `{1}` is disabled")]
  NoAbilityAction(String, String),
  #[error("no jwt secret")]
  NoJwtSecret,
  #[error("invalid jwt")]
  InvalidJwt,
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
  #[error("could not add mail form template: reason = `{0}`")]
  MailFormAddTemplateFailed(String),
  #[error("could not render mail form: reason = `{0}`")]
  MailFormRenderFailed(String),
  #[error("could not parse email address")]
  EmailParseFailed,
  #[error("could not build a e-mail message: reason = `{0}`")]
  EmailMessageBuildFailed(String),
  #[error("could not send email: reason = `{0}`")]
  EmailSendFailed(String),
  #[error("invalid verification code")]
  VerificationFailed,

  // runtime errors
  #[error("reason = {0}")]
  Unsupported(String),
  #[error("reason = {0}")]
  Timeout(String),
  #[error("reason = {0}")]
  Busy(String),
  #[error("client disconnected")]
  Disconnected,

  // external crate errors
  #[error("status = {0}, reason = {1}")]
  DatabaseError(u16, String),
  #[cfg(feature = "aws-s3")]
  #[error("status = {0}, reason = {1}")]
  AwsS3SdkError(u16, String),
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
      | SumidaError::AccountBlocked
      | SumidaError::NotAuthorized(_)
      | SumidaError::NoCredentials
      | SumidaError::NoUserOrganization(_)
      | SumidaError::NoUserRole(_)
      | SumidaError::InvalidJwt
      | SumidaError::NoJwtSecret
      | SumidaError::InvalidApiKey
      | SumidaError::InvalidAuthorizationToken
      | SumidaError::OnlyAdminAllowed
      | SumidaError::VerificationFailed => Ok(StatusCode::UNAUTHORIZED),
      SumidaError::NoAbilityTopic(_) | SumidaError::NoAbilityAction(_, _) => Ok(StatusCode::NOT_ACCEPTABLE),
      SumidaError::InvalidAuthorizationMethod(_) => Ok(StatusCode::NOT_IMPLEMENTED),

      // runtime errors
      SumidaError::Unsupported(_) => Ok(StatusCode::NOT_IMPLEMENTED),
      SumidaError::Timeout(_) => Ok(StatusCode::REQUEST_TIMEOUT),
      SumidaError::Busy(_) => Ok(StatusCode::TOO_MANY_REQUESTS),

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
    let status_code: StatusCode = self.clone().into();
    let reason = self.to_string();
    tracing::error!("SumidaError: {}, {}", status_code, reason);

    (
      status_code,
      if cfg!(debug_assertions) {
        Json(json!({
          "statusCode": status_code.as_u16(),
          "reason": reason,
        }))
      } else {
        Json(json!({
          "statusCode": status_code.as_u16(),
        }))
      },
    )
      .into_response()
  }
}

impl<T> From<(StatusCode, T)> for SumidaError
where
  T: ToString,
{
  fn from(value: (StatusCode, T)) -> Self {
    SumidaError::Unknown(value.0.as_u16(), value.1.to_string())
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
