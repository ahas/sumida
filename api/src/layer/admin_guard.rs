use axum::{
  body::Body,
  extract::FromRequestParts,
  http::{Request, Response},
  response::IntoResponse,
};
use error::SumidaError;
use futures::future::BoxFuture;
use infra::session::UserSession;
use std::task::{Context, Poll};
use tower::{Layer, Service};

use crate::SumidaState;

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
    let _state = self.state.clone();
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

      match session.profile.is_admin {
        true => {
          let req = Request::from_parts(parts, body);
          inner.call(req).await
        }
        false => {
          let error_response = SumidaError::OnlyAdminAllowed.into_response();
          Ok(error_response)
        }
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
