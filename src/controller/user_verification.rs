use axum::extract::State;
use axum::{routing, Json, Router};

use crate::core::error::AppError;
use crate::service::prelude::*;

async fn send<S>(
  State(_state): State<S>,
  Json(body): Json<SendUserVerification>,
) -> Result<UserVerificationSent, AppError> {
  UserVerificationService::send(body).await?;

  Ok(UserVerificationSent)
}

pub fn new<S>(_state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  Router::<S>::new().route("/", routing::post(send::<S>))
}
