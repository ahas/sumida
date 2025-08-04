use axum::{Extension, Router, extract::connect_info::IntoMakeServiceWithConnectInfo, routing};
use reqwest::{Client, StatusCode};
use std::{net::SocketAddr, time::Duration};
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
  // shutdown_tx: Option<oneshot::Sender<()>>,
  // handle: Option<JoinHandle<anyhow::Result<()>>>,
}

impl TestServer {
  pub async fn spawn() -> anyhow::Result<Self> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let addr = listener.local_addr()?;
    let app = build_app().await?;

    tokio::spawn(async move {
      axum::serve(listener, app).await.unwrap();
    });

    // let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    // let handle = tokio::spawn(async move {
    //   let server = axum::serve(listener, app).with_graceful_shutdown(async move {
    //     let _ = shutdown_rx.await;
    //   });
    //
    //   server.await.map_err(|err| err.into())
    // });

    let server = Self {
      addr,
      // shutdown_tx: Some(shutdown_tx),
      // handle: Some(handle),
    };

    // server.wait_ready().await?;

    Ok(server)
  }

  async fn wait_ready(&self) -> anyhow::Result<()> {
    let client = Client::builder().connect_timeout(Duration::from_secs(60)).build()?;

    let url = format!("http://{}/health", self.addr);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);

    loop {
      match client.get(&url).send().await {
        Ok(res) if res.status().is_success() => return Ok(()),
        _ => {
          if std::time::Instant::now() > deadline {
            anyhow::bail!("server not ready at {url}");
          }

          tokio::time::sleep(Duration::from_millis(50)).await;
        }
      }
    }
  }

  // pub async fn stop(mut self) {
  //   if let Some(tx) = self.shutdown_tx.take() {
  //     let _ = tx.send(());
  //   }
  //   if let Some(handle) = self.handle.take() {
  //     let _ = handle.await;
  //   }
  // }
}

// impl Drop for TestServer {
//   fn drop(&mut self) {
//     if let Some(tx) = self.shutdown_tx.take() {
//       let _ = tx.send(());
//     }
//   }
// }

async fn build_app() -> anyhow::Result<IntoMakeServiceWithConnectInfo<Router, SocketAddr>> {
  let settings = infra::settings::get();
  let state = TestServerState;
  let (messenger_layer, messenger) = infra::messenger::new_layer();

  let api = Router::new()
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
    .layer(settings.session.ip_source.clone().into_extension());

  let app = Router::new()
    .route("/health", routing::get(|| async { (StatusCode::OK, "ok") }))
    .merge(api)
    .into_make_service_with_connect_info::<SocketAddr>();

  Ok(app)
}
