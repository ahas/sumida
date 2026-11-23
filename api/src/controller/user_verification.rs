use axum::extract::State;
use axum::{Json, Router, routing};

use super::ApiController;
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{ErrorBody, Unreadable, WrongBody};
use crate::service::prelude::*;

/// Where this controller's routes stand. The router nests here and the document appends here, so
/// the prefix is written once and the two cannot drift apart.
pub const MOUNT: &str = "/user-verifications";

/// What binds this controller's operations together in the document.
#[cfg(feature = "openapi")]
const TAG: &str = "user verification";

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
}

/// Sends an account a six-digit code and stores it against the `kind` that was named.
///
/// No credentials are needed to call this. The code is good for three minutes; while one is still
/// good, asking again sends nothing, so a second request is not a second code. Asking is limited per
/// account and per address (`security.verification.issue_limit` per `window`), and past that the
/// answer is `429`; it is counted for an account that does not exist just the same.
///
/// **The answer says nothing about what happened.** An account that is not there, an account with
/// no address to send to, and a code that was withheld because one is already out all answer
/// exactly as a code that was just sent — otherwise this route would tell anyone who asks which
/// accounts exist and which are reachable. So a caller learns whether a code arrived only from the
/// person who received it.
///
/// `kind` is what the code will be spent on and has to match at the far end;
/// `POST /users/change-password` spends one of kind `password-change`. `method` names the channel:
/// `email` is the only one. A code is stored only together with the mail that carries it, so asking
/// over any other channel, or of a deployment built without mail, makes no code at all.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_verification_send",
  request_body = SendUserVerification,
  security(()),
  responses(
    (
      status = 201,
      description = "The request was taken. The body is the JSON literal `null`, and it is the same \
                     answer whether a code was sent, withheld, or had nowhere to go.",
      content_type = "application/json",
    ),
    Unreadable,
    WrongBody,
    (
      status = 429,
      description = "Too many codes were asked for this account, or from this address, in the \
                     current window.",
      body = ErrorBody,
      content_type = "application/json",
    ),
  ),
))]
async fn send<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<SendUserVerification>,
) -> Result<UserVerificationSent, SumidaError> {
  UserVerificationService::new(state, session).send(body).await?;

  Ok(UserVerificationSent)
}

impl<S> ApiController<S> for UserVerificationController<S>
where
  S: SumidaState,
{
  fn router(self, _state: &S) -> axum::Router<S> {
    Router::new() //
      .route("/", routing::post(send::<S>))
  }
}
