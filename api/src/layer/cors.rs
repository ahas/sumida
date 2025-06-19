use axum::http::{Method, header};
use core::settings;
use tower_http::cors::CorsLayer;

pub fn new() -> CorsLayer {
  let settings = settings::get();
  let origins = settings
    .server
    .cors
    .as_ref()
    .map_or_else(Vec::new, |x| {
      x.allow_origins
        .clone()
        .into_iter()
        .filter_map(|v| v.parse().ok())
        .collect()
    });

  tracing::info!("allowed cors origins:");

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
