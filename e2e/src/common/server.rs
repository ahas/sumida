use axum::{Extension, Router, extract::connect_info::IntoMakeServiceWithConnectInfo};
use std::net::SocketAddr;
use tokio::{
  sync::{OnceCell, oneshot},
  task::JoinHandle,
};

static SERVER: OnceCell<TestServer> = OnceCell::const_new();

pub async fn use_server() -> &'static TestServer {
  SERVER.get_or_init(|| async { TestServer::spawn().await.expect("Failed to spawn the server") }).await
}

#[derive(Clone)]
pub struct TestServerState;

pub struct TestServer {
  pub addr: SocketAddr,
  shutdown_tx: Option<oneshot::Sender<()>>,
  join: JoinHandle<anyhow::Result<()>>,
}

impl TestServer {
  pub async fn spawn() -> anyhow::Result<Self> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let app = build_app().await?;
    let addr = listener.local_addr()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = axum::serve(listener, app).with_graceful_shutdown(async {
      let _ = shutdown_rx.await;
    });

    let join = tokio::spawn(async move {
      server.await?;
      Ok::<_, anyhow::Error>(())
    });

    Ok(Self {
      addr,
      shutdown_tx: Some(shutdown_tx),
      join,
    })
  }

  pub async fn stop(mut self) {
    if let Some(tx) = self.shutdown_tx.take() {
      let _ = tx.send(());
    }
    let _ = self.join.await;
  }
}

async fn build_app() -> anyhow::Result<IntoMakeServiceWithConnectInfo<Router, SocketAddr>> {
  let settings = infra::settings::get();
  let state = TestServerState;
  let (messenger_layer, messenger) = infra::messenger::new_layer();

  let app = Router::new()
    .merge(
      api::Sumida::new() //
        .set_state(&state)
        .router(),
    )
    .layer(messenger_layer)
    .layer(
      api::layer::session::new(infra::session::redis_store::new(&settings.session.redis).await?).await?,
    )
    .layer(api::layer::cors::new())
    .layer(Extension(messenger))
    .layer(Extension(settings))
    .with_state(state)
    .layer(settings.session.ip_source.clone().into_extension())
    .into_make_service_with_connect_info::<SocketAddr>();

  Ok(app)
}
