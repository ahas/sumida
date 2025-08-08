use axum::extract::State;
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::service::prelude::*;

use super::ApiController;

#[derive(Clone, Debug, Default)]
pub struct UserVerificationController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> UserVerificationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn send(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<SendUserVerification>,
  ) -> Result<UserVerificationSent, SumidaError> {
    UserVerificationService::new(state, session).send(body).await?;

    Ok(UserVerificationSent)
  }
}

impl<S> ApiController<S> for UserVerificationController<S>
where
  S: SumidaState,
{
  fn router(self, _state: &S) -> axum::Router<S> {
    Router::new() //
      .route("/", routing::post(Self::send))
  }
}
