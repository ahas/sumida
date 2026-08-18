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

      // `service:<namespace>#can_<action>`. Between §14 steps 7 and 13 this went through
      // `ap::ns(..)`, whose shim decided per namespace whether to answer from the compiled rule or
      // from `access_policies`; once all four products were routed there was one store and no
      // decision left to make, so step 14 deleted the shim and the call is made here directly.
      //
      // A rule that does not compile refuses the request rather than admitting it. The error is
      // logged because a gate that fails closed and says nothing is indistinguishable from a
      // caller who simply holds no grant.
      let permission = format!("can_{action}");
      let subject = SubjectSet::of(&session);
      let is_allowed =
        match Authz::check_object("service", &namespace, &permission, &subject, state.db()).await {
          Ok(allowed) => allowed,
          Err(error) => {
            tracing::error!("authz: `service:{namespace}#{permission}` did not evaluate: {error}");
            false
          }
        };

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
// compilation when used; with zero call sites it was removed. Per-board permissions are a rule now
// (`board_post#edit` unions its group, its board and its author, and subtracts `blocked`), so there
// is nothing left for a per-board layer to compose at the call site.
