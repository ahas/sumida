use axum::extract::State;
use axum::{Json, Router, routing};

use crate::service::prelude::*;

use super::ApiController;

pub struct UserVerificationController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> UserVerificationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn send(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<SendUserVerification>,
  ) -> Result<UserVerificationSent, SumidaError> {
    UserVerificationService::new(user_ctx).send(body).await?;

    Ok(UserVerificationSent)
  }
}

impl<S> ApiController<S> for UserVerificationController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, _state: &S) -> axum::Router<S> {
    Router::new() //
      .route("/", routing::post(Self::send))
  }
}
