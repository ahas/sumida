use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::{Extension, Router, routing};
use sea_orm::DatabaseConnection;
use sumida::api::SumidaState;
use sumida::infra::session::redis_store::RedisSessionStore;
use tokio::signal;
use tower_http::trace::TraceLayer;

include!(concat!(env!("OUT_DIR"), "/version.rs"));

#[derive(Clone)]
pub struct BasicState {
  db: DatabaseConnection,
}

impl SumidaState for BasicState {
  fn db(&self) -> &DatabaseConnection {
    &self.db
  }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
  color_eyre::install().expect("failed to install color_eyre");

  dotenvy::dotenv().ok();
  let settings = sumida::infra::settings::get();

  sumida::infra::tracing::subscribe();
  let db = sumida::infra::db::connect().await;

  tracing::info!("version: {}", VERSION);

  let state = BasicState { db };

  let (messenger_layer, messenger) = sumida::infra::messenger::new_layer();
  let redis_settings = settings
    .session
    .redis
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("missing `session.redis` in settings"))?;
  let redis_store = RedisSessionStore::new(redis_settings).await?;
  let redis_store_handle = redis_store.handle.clone();

  let app = Router::new()
    .merge(
      sumida::api::Sumida::new() //
        .set_state(&state)
        .router(),
    )
    .layer(messenger_layer)
    .layer(sumida::api::layer::session::new(redis_store.store()).await?)
    .layer(sumida::api::layer::cors::new())
    .layer(Extension(messenger))
    .layer(Extension(settings))
    .with_state(state)
    .route("/", routing::get(health_check))
    .route("/", routing::post(health_check))
    .layer(settings.session.ip_source.clone().into_extension())
    .layer(TraceLayer::new_for_http())
    .into_make_service_with_connect_info::<SocketAddr>();

  let host = &settings.server.host;
  let listener = tokio::net::TcpListener::bind(host).await.unwrap();

  tracing::info!("listening on {}", listener.local_addr().unwrap());
  axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal({
      let redis_store_handle = redis_store_handle.clone();

      move || {
        let redis_store_handle = redis_store_handle.lock().unwrap();
        redis_store_handle.abort_handle().abort();
      }
    }))
    .await?;

  Ok(())
}

async fn shutdown_signal<F: Fn()>(cb: F) {
  let ctrl_c = async {
    signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
  };

  #[cfg(unix)]
  let terminate = async {
    signal::unix::signal(signal::unix::SignalKind::terminate())
      .expect("failed to install signal handler")
      .recv()
      .await;
  };

  #[cfg(not(unix))]
  let terminate = std::future::pending::<()>();

  tokio::select! {
      _ = ctrl_c => { cb(); },
      _ = terminate => { cb(); },
  }
}

async fn health_check() -> StatusCode {
  StatusCode::OK
}
