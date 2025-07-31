use axum::{Extension, Router, routing};
use reqwest::{Method, RequestBuilder, StatusCode, Url};
use std::{net::SocketAddr, time::Duration};
use tokio::sync::{OnceCell, oneshot};

use super::db::TestDb;

static SERVER: OnceCell<TestServer> = OnceCell::const_new();

pub async fn use_server() -> &'static TestServer {
  SERVER
    .get_or_init(|| async {
      //
      TestServer::spawn().await.expect("Failed to spawn the server")
    })
    .await
}

#[derive(Clone)]
pub struct TestServerState;

pub struct TestServer {
  pub client: reqwest::Client,
  pub addr: SocketAddr,
  _shutdown: oneshot::Sender<()>,
}

impl TestServer {
  pub async fn spawn() -> anyhow::Result<Self> {
    dotenvy::from_filename(".env.e2e").expect("Failed to load .env.e2e");

    let settings = infra::settings::get();
    infra::db::establish_connection().await;
    TestDb::setup().await?;

    let _ = tracing_subscriber::fmt()
      .with_test_writer()
      .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
      .try_init();

    let state = TestServerState;

    let (messenger_layer, messenger) = infra::messenger::new_layer();
    let app = Router::new()
      .route("/health", routing::get(|| async { StatusCode::NO_CONTENT }))
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
      let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
        tokio::time::sleep(Duration::from_millis(50)).await;
      });

      if let Err(e) = server.await {
        tracing::error!(error = ?e, "axum server error");
      }
    });

    let client = reqwest::Client::builder() //
      .cookie_store(true)
      .redirect(reqwest::redirect::Policy::none())
      .timeout(Duration::from_secs(10))
      .connect_timeout(Duration::from_secs(3))
      .build()?;

    let this = Self {
      addr,
      client,
      _shutdown: shutdown_tx,
    };
    this.wait_ready().await?;

    Ok(this)
  }

  pub async fn wait_ready(&self) -> anyhow::Result<()> {
    let url = self.url("/health");

    for _ in 0..50 {
      match self.client.get(url.clone()).send().await {
        Ok(res) if res.status().is_success() => return Ok(()),
        _ => tokio::time::sleep(Duration::from_millis(100)).await,
      }
    }

    anyhow::bail!("server not ready after retries");
  }

  // Basic HTTP method helpers
  #[inline]
  pub fn base_url(&self) -> Url {
    Url::parse(&format!("http://{}/", self.addr)).unwrap()
  }

  #[inline]
  pub fn url(&self, path: &str) -> Url {
    self.base_url().join(path.trim_start_matches('/')).unwrap()
  }

  #[inline]
  fn request(&self, method: Method, path: &str) -> RequestBuilder {
    self.client.request(method, self.url(path))
  }

  #[inline]
  pub fn get(&self, path: &str) -> RequestBuilder {
    self.request(Method::GET, path)
  }

  #[inline]
  pub fn post(&self, path: &str) -> RequestBuilder {
    self.request(Method::POST, path)
  }

  #[inline]
  pub fn patch(&self, path: &str) -> RequestBuilder {
    self.request(Method::PATCH, path)
  }

  #[inline]
  pub fn delete(&self, path: &str) -> RequestBuilder {
    self.request(Method::DELETE, path)
  }

  // Json body helpers
  #[inline]
  pub fn post_json<T: serde::Serialize + ?Sized>(&self, path: &str, body: &T) -> RequestBuilder {
    self.post(path).json(body)
  }

  #[inline]
  pub fn patch_json<T: serde::Serialize + ?Sized>(&self, path: &str, body: &T) -> RequestBuilder {
    self.patch(path).json(body)
  }

  #[inline]
  pub fn delete_json<T: serde::Serialize + ?Sized>(&self, path: &str, body: &T) -> RequestBuilder {
    self.delete(path).json(body)
  }
}
