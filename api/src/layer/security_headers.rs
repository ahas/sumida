//! Headers every API response carries for the browser's sake.
//!
//! The API answers JSON and files to the screens of other origins, so it asks browsers to keep to
//! HTTPS for this host, not to guess a response's type, not to frame it and not to pass the full
//! address on. A handler that sets one of these itself keeps its own value. `includeSubDomains` is
//! left out: the API's host does not speak for every host of its domain.

use axum::http::HeaderValue;
use axum::http::header::{
  REFERRER_POLICY,
  STRICT_TRANSPORT_SECURITY,
  X_CONTENT_TYPE_OPTIONS,
  X_FRAME_OPTIONS,
};
use tower::ServiceBuilder;
use tower::layer::util::{Identity, Stack};
use tower_http::set_header::SetResponseHeaderLayer;

type Set = SetResponseHeaderLayer<HeaderValue>;

/// The layer that adds the headers to every response that does not already carry them.
pub fn new() -> ServiceBuilder<Stack<Set, Stack<Set, Stack<Set, Stack<Set, Identity>>>>> {
  ServiceBuilder::new()
    .layer(SetResponseHeaderLayer::if_not_present(
      STRICT_TRANSPORT_SECURITY,
      HeaderValue::from_static("max-age=31536000"),
    ))
    .layer(SetResponseHeaderLayer::if_not_present(
      X_CONTENT_TYPE_OPTIONS,
      HeaderValue::from_static("nosniff"),
    ))
    .layer(SetResponseHeaderLayer::if_not_present(
      X_FRAME_OPTIONS,
      HeaderValue::from_static("DENY"),
    ))
    .layer(SetResponseHeaderLayer::if_not_present(
      REFERRER_POLICY,
      HeaderValue::from_static("strict-origin-when-cross-origin"),
    ))
}

#[cfg(test)]
mod tests {
  use axum::Router;
  use axum::body::Body;
  use axum::http::{Request, StatusCode};
  use axum::routing::get;
  use tower::ServiceExt;

  #[tokio::test]
  async fn every_response_carries_the_headers_and_a_handlers_own_value_stays() {
    let app = Router::new()
      .route("/", get(|| async { "ok" }))
      .route("/framed", get(|| async { ([("x-frame-options", "SAMEORIGIN")], "ok") }))
      .layer(super::new());

    let plain = app.clone().oneshot(Request::get("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(plain.status(), StatusCode::OK);
    assert_eq!(plain.headers()["strict-transport-security"], "max-age=31536000");
    assert_eq!(plain.headers()["x-content-type-options"], "nosniff");
    assert_eq!(plain.headers()["x-frame-options"], "DENY");
    assert_eq!(plain.headers()["referrer-policy"], "strict-origin-when-cross-origin");

    let framed = app.oneshot(Request::get("/framed").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(framed.headers()["x-frame-options"], "SAMEORIGIN");
  }
}
