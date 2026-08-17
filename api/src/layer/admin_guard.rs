use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::{Request, Response};
use axum::response::IntoResponse;
use error::SumidaError;
use futures::future::BoxFuture;
use infra::session::UserSession;
use tower::{Layer, Service};

use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;

#[derive(Clone)]
pub struct AdminGuardLayer<S> {
  state: S,
}

impl<S> AdminGuardLayer<S> {
  pub fn new(state: S) -> Self {
    Self { state }
  }
}

impl<S, T> Layer<T> for AdminGuardLayer<S>
where
  S: Clone,
{
  type Service = AdminGuardService<S, T>;

  fn layer(&self, inner: T) -> Self::Service {
    AdminGuardService {
      state: self.state.clone(),
      inner,
    }
  }
}

#[derive(Clone)]
pub struct AdminGuardService<S, T> {
  state: S,
  inner: T,
}

impl<S, T> Service<Request<Body>> for AdminGuardService<S, T>
where
  S: SumidaState,
  T: Service<Request<Body>, Response = Response<Body>, Error = std::convert::Infallible>
    + Clone
    + Send
    + 'static,
  T::Future: Send + 'static,
{
  type Response = Response<Body>;
  type Error = std::convert::Infallible;
  type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    self.inner.poll_ready(cx)
  }

  fn call(&mut self, req: Request<Body>) -> Self::Future {
    let state = self.state.clone();
    let mut inner = self.inner.clone();

    Box::pin(async move {
      let (mut parts, body) = req.into_parts();

      let session = match UserSession::from_request_parts(&mut parts, &()).await {
        Ok(session) => session,
        Err(_) => {
          let error_response = SumidaError::NoCredentials.into_response();
          return Ok(error_response);
        }
      };

      // R1 through the rule, not through the session field (design §8.1).
      //
      // `session.profile.is_admin` was filled in from the key's owner by `verify_api_key`, so a
      // **secured** API key — one that is supposed to carry only its own grants — passed this guard
      // with its owner's administrative power. `SubjectSet::of` gives a secured key an empty
      // `user_ids`, so `platform#flagged` cannot match `users.is_admin` at all and the escalation
      // is closed structurally rather than by remembering to check `api_key.is_secured` here.
      //
      // For an interactive session the answer is identical to the field: `flagged` reads the same
      // column for the same user id.
      let subject = SubjectSet::of(&session);
      let allowed = match Authz::is_superuser(&subject, state.db()).await {
        Ok(allowed) => allowed,
        // The rule set did not compile or the statement failed. Refuse: an administrative route
        // that opens when the evaluator is broken is the one failure mode this guard exists to
        // prevent.
        Err(error) => {
          tracing::error!("admin guard could not evaluate `platform:root#superuser`: {error}");
          false
        }
      };

      if allowed {
        let req = Request::from_parts(parts, body);
        inner.call(req).await
      } else {
        let error_response = SumidaError::OnlyAdminAllowed.into_response();
        Ok(error_response)
      }
    })
  }
}

#[macro_export]
macro_rules! admin_guard {
  ($state:expr) => {
    $crate::layer::AdminGuardLayer::new($state.clone())
  };
}
