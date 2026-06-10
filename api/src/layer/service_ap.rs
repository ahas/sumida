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

#[derive(Clone)]
pub struct ServiceApLayer<S> {
  state: S,
  namespace: String,
  action: String,
}

impl<S> ServiceApLayer<S> {
  pub fn new(state: S, namespace: impl Into<String>, action: impl Into<String>) -> Self {
    Self {
      state,
      namespace: namespace.into(),
      action: action.into(),
    }
  }
}

impl<S, T> Layer<T> for ServiceApLayer<S>
where
  S: Clone,
{
  type Service = ServiceApService<S, T>;

  fn layer(&self, inner: T) -> Self::Service {
    ServiceApService {
      state: self.state.clone(),
      namespace: self.namespace.clone(),
      action: self.action.clone(),
      inner,
    }
  }
}

#[derive(Clone)]
pub struct ServiceApService<S, T> {
  state: S,
  namespace: String,
  action: String,
  inner: T,
}

impl<S, T> Service<Request<Body>> for ServiceApService<S, T>
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
    let namespace = self.namespace.clone();
    let action = self.action.clone();
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

      let req = Request::from_parts(parts, body);

      use crate::util::ap::{self, ApGuard};
      let is_allowed = ap::ns(namespace)
        .object("service")
        .action(action)
        .who(&session)
        .check_allowed(state.db())
        .await;

      if is_allowed {
        inner.call(req).await
      } else {
        let error_response = SumidaError::NoServiceAvailable.into_response();
        Ok(error_response)
      }
    })
  }
}

#[macro_export]
macro_rules! service_ap_new {
  ($state:expr, $ns:literal, $action:ident) => {
    $crate::layer::ServiceApLayer::new($state.clone(), $ns, stringify!($action))
  };
}

#[macro_export]
macro_rules! service_ap {
  ($state:expr, $ns:literal, $action:ident) => {
    $crate::layer::ServiceApLayer::new($state.clone(), $ns, stringify!($action))
  };
}

// NOTE: The former `board_ap!` macro referenced the unimplemented `BoardApLayer`, so it broke
// compilation when used; with zero call sites it was removed. Per-board permissions are handled
// by each service's internal `ap::chain`/ownership checks (e.g. board_post's `authorize_post_mutation`).
// If you need to extract board_id, use `layer::board_ap::helpers`.
