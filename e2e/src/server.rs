use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use api::SumidaState;
use axum::{Extension, Router, routing};
use infra::session::redis_store::RedisSessionStore;
use infra::settings;
use reqwest::{Client, StatusCode};
use sea_orm::DatabaseConnection;
use tokio::sync::{Mutex, OnceCell, oneshot};
use tokio::task::JoinHandle;

static SERVER: OnceCell<TestServer> = OnceCell::const_new();

pub async fn use_server() -> &'static TestServer {
  SERVER
    .get_or_init(|| async { TestServer::spawn().await.expect("Failed to spawn the server") })
    .await
}

#[derive(Clone)]
pub struct TestServerState {
  db: DatabaseConnection,
}

impl SumidaState for TestServerState {
  fn db(&self) -> &DatabaseConnection {
    &self.db
  }
}

#[allow(dead_code)]
pub struct TestServer {
  pub addr:    SocketAddr,
  shutdown_tx: Option<oneshot::Sender<()>>,
  handle:      Option<JoinHandle<Result<(), anyhow::Error>>>,
  redis_store: Arc<Mutex<RedisSessionStore>>,
}

impl TestServer {
  pub async fn spawn() -> anyhow::Result<Self> {
    let settings = settings::get();
    let db = infra::db::connect().await;
    let listener = tokio::net::TcpListener::bind(&settings.server.host).await?;
    let addr = listener.local_addr()?;

    let state = TestServerState { db };
    let (messenger_layer, messenger) = infra::messenger::new_layer();
    let redis_store =
      Arc::new(Mutex::new(RedisSessionStore::new(settings.session.redis.as_ref().unwrap()).await?));

    let api = Router::new()
      .merge(
        api::Sumida::new() //
          .set_state(&state)
          .router(),
      )
      .layer(messenger_layer)
      .layer(
        api::layer::session::new({
          let store = redis_store.clone();
          let store_guard = store.lock().await;
          store_guard.store()
        })
        .await?,
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

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
      axum::serve(listener, app)
        .with_graceful_shutdown(async {
          shutdown_rx.await.ok();
        })
        .await?;

      Ok::<(), anyhow::Error>(())
    });

    let server = Self {
      addr,
      shutdown_tx: Some(shutdown_tx),
      handle: Some(handle),
      redis_store: redis_store.clone(),
    };
    server.wait_ready().await?;

    Ok(server)
  }

  pub async fn stop(&mut self) {
    if let Some(tx) = self.shutdown_tx.take() {
      let _ = tx.send(());
    }

    // if let Some(handle) = self.handle.take() {
    //   let _ = handle.await;
    // }

    let redis_store = self.redis_store.lock().await;
    let redis_conn = redis_store.handle.lock().unwrap();
    redis_conn.abort_handle().abort();
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
}
