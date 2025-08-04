use axum::http::StatusCode;
use axum::{Extension, Router, routing};
use std::net::SocketAddr;
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
  let app = Router::new()
    .merge(
      sumida::api::Sumida::new() //
        .set_state(&state)
        .router(),
    )
    .layer(messenger_layer)
    .layer(
      sumida::api::layer::session::new(
        sumida::infra::session::redis_store::new(&settings.session.redis).await?,
      )
      .await?,
    )
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
  axum::serve(listener, app).await?;

  Ok(())
}

async fn health_check() -> StatusCode {
  StatusCode::OK
}
