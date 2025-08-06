use axum::http::StatusCode;
use axum::{Extension, Router, routing};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use sumida::infra::session::redis_store::RedisSessionStore;
use tokio::signal;
use tower_http::trace::TraceLayer;

include!(concat!(env!("OUT_DIR"), "/version.rs"));

#[derive(Clone)]
pub struct BasicState;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
  color_eyre::install().expect("failed to install color_eyre");

  dotenvy::dotenv().ok();
  let settings = sumida::infra::settings::get();

  sumida::infra::tracing::subscribe();
  sumida::infra::db::establish_connection().await;

  tracing::info!("version: {}", VERSION);

  let state = BasicState;

  let (messenger_layer, messenger) = sumida::infra::messenger::new_layer();
  let redis_store = Arc::new(Mutex::new(RedisSessionStore::new(&settings.session.redis).await?));

  let app = Router::new()
    .merge(
      sumida::api::Sumida::new() //
        .set_state(&state)
        .router(),
    )
    .layer(messenger_layer)
    .layer(sumida::api::layer::session::new(redis_store.clone().lock().unwrap().store()).await?)
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
      let redis_store = redis_store.clone();

      move || {
        let redis_store = redis_store.lock().unwrap();
        let redis_store_handle = redis_store.handle.lock().unwrap();
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
