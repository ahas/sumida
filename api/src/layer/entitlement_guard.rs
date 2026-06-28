use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::{Request, Response};
use axum::response::IntoResponse;
use entity::membership::OwnerKind;
use error::SumidaError;
use futures::future::BoxFuture;
use infra::session::UserSession;
use tower::{Layer, Service};

use crate::SumidaState;
use crate::service::membership::entitlement::EntitlementService;

/// Route-level entitlement gate: before the handler runs, requires that the
/// session user (personal owner) is entitled to `action` — via an active
/// subscription or by consuming credits, per the configured gate order.
///
/// Use this for static, resource-independent actions. Because it runs in its own
/// transaction *before* the handler, a credit consumed here is **not** rolled
/// back if the handler later fails. For resource-scoped or transactional gates,
/// call [`EntitlementService::require_action`] inside the handler's transaction
/// instead.
#[derive(Clone)]
pub struct EntitlementLayer<S> {
  state: S,
  action: String,
}

impl<S> EntitlementLayer<S> {
  pub fn new(state: S, action: impl Into<String>) -> Self {
    Self {
      state,
      action: action.into(),
    }
  }
}

impl<S, T> Layer<T> for EntitlementLayer<S>
where
  S: Clone,
{
  type Service = EntitlementGuardService<S, T>;

  fn layer(&self, inner: T) -> Self::Service {
    EntitlementGuardService {
      state: self.state.clone(),
      action: self.action.clone(),
      inner,
    }
  }
}

#[derive(Clone)]
pub struct EntitlementGuardService<S, T> {
  state: S,
  action: String,
  inner: T,
}

impl<S, T> Service<Request<Body>> for EntitlementGuardService<S, T>
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
    let action = self.action.clone();
    let mut inner = self.inner.clone();

    Box::pin(async move {
      let (mut parts, body) = req.into_parts();

      let session = match UserSession::from_request_parts(&mut parts, &()).await {
        Ok(session) => session,
        Err(_) => return Ok(SumidaError::NoCredentials.into_response()),
      };

      let req = Request::from_parts(parts, body);

      let outcome = EntitlementService::new(state, session)
        .require_action(&action, OwnerKind::Personal, None, None)
        .await;

      match outcome {
        Ok(_) => inner.call(req).await,
        Err(e) => Ok(e.into_response()),
      }
    })
  }
}

/// Route-level entitlement gate for a static action (personal owner).
///
/// `entitlement_guard!(state, "premium-export")` — requires an active
/// subscription covering the action, or consumes the action's credit cost.
#[macro_export]
macro_rules! entitlement_guard {
  ($state:expr, $action:literal) => {
    $crate::layer::EntitlementLayer::new($state.clone(), $action)
  };
}
