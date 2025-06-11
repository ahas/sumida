use axum::extract::State;
use axum::{Json, Router, routing};

use crate::service::prelude::*;

async fn send<S>(
  State(_state): State<S>,
  Json(body): Json<SendUserVerification>,
) -> Result<UserVerificationSent, SumidaError> {
  UserVerificationService::send(body).await?;

  Ok(UserVerificationSent)
}

pub fn new<S>(_state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  Router::<S>::new().route("/", routing::post(send::<S>))
}
