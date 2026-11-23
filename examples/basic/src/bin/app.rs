use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::{Extension, Router, routing};
use sea_orm::DatabaseConnection;
use sumida::api::SumidaState;
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

  // Membership background maintenance: expire due subscriptions, sweep lapsed
  // credit lots, and send expiring-soon / low-balance notices on a fixed cadence.
  sumida::api::service::membership::maintenance::spawn(state.clone(), std::time::Duration::from_secs(3600));

  let (messenger_layer, messenger) = sumida::infra::messenger::new_layer(state.db.clone());
  // Every AWS client sumida builds, the session store's among them, reads this one shared SDK
  // configuration, so it is loaded before any of them.
  #[cfg(feature = "full-aws")]
  sumida::infra::aws::load_sdk_config().await;
  let session_store = sumida::infra::session::new_store(&settings.session).await?;

  let app = Router::new()
    .merge(
      // 이 예제는 sumida 자신의 규칙만 쓴다 — 자기 규칙이 없다는 선언을 명시적으로 한다.
      sumida::api::Sumida::new(&[]) //
        .set_state(&state)
        .router(),
    )
    .layer(messenger_layer)
    .layer(sumida::api::layer::session::new(session_store).await?)
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
  axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;

  Ok(())
}

async fn shutdown_signal() {
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
      _ = ctrl_c => {},
      _ = terminate => {},
  }
}

async fn health_check() -> StatusCode {
  StatusCode::OK
}
