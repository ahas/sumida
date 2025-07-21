# SUMIDA

Scaleable Utility Modules Integration for Dynamic Application

### Usage

```rust
use axum::body::{Body, Bytes};
use axum::http::StatusCode;
use chrono::format;
use color_eyre::owo_colors::DynColor;
use app_server::app::provider::role_provider::{AppRoleAbility, AppRoleProvider};
use app_server::app::{self, AppState};

use axum::{extract::State, routing, Extension, Router};
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::trace::TraceLayer;
use tracing::Span;

include!(concat!(env!("OUT_DIR"), "/version.rs"));

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
  color_eyre::install().expect("failed to install color_eyre");

  dotenv::dotenv().ok();

  let settings = sumida::core::settings::get();
  let state = AppState;

  sumida::core::tracing::subscribe();
  sumida::core::db::establish_connection().await;
  sumida::core::aws::load_sdk_config().await;

  tracing::info!("version: {}", VERSION);

  let app = Router::new()
    .merge(Router::<AppState>::from(
      sumida::SumidaRouter::new()
        .set_state(&state)
    ))
    .merge(app::new_router(&state).await?)
    .layer(sumida::core::session::new_layer().await?)
    .layer(sumida::core::cors::new_layer())
    .layer(Extension(settings))
    .with_state(state)
    .route("/", routing::get(health_check))
    .route("/", routing::post(health_check))
    .layer(TraceLayer::new_for_http());

  let host = &settings.server.host;
  let listener = tokio::net::TcpListener::bind(host).await.unwrap();

  tracing::info!("listening on {}", listener.local_addr().unwrap());
  axum::serve(listener, app.into_make_service()).await?;

  Ok(())
}

async fn health_check() -> StatusCode {
  StatusCode::OK
}
```
