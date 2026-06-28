use axum::http::{Method, header};
use infra::settings;
use tower_http::cors::CorsLayer;

pub fn new() -> CorsLayer {
  let settings = settings::get();
  let origins = settings.server.cors.as_ref().map_or_else(Vec::new, |x| {
    x.allow_origins
      .clone()
      .into_iter()
      .filter(|v| {
        // allow_credentials(true) combined with a wildcard '*' origin is an invalid combination that browsers reject.
        // Ignore it explicitly to avoid the misconception that a misconfiguration opens up all origins.
        if v == "*" {
          tracing::warn!(
            "CORS allow_origins '*' is ignored: it cannot be combined with allow_credentials=true"
          );
          false
        } else {
          true
        }
      })
      .filter_map(|v| v.parse().ok())
      .collect()
  });

  CorsLayer::new()
    .allow_credentials(true)
    .allow_origin(origins)
    .allow_methods([
      Method::HEAD,
      Method::OPTIONS,
      Method::GET,
      Method::POST,
      Method::PATCH,
      Method::PUT,
      Method::DELETE,
    ])
    .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE])
}
