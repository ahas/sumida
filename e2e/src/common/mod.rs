use axum::{Extension, Router, routing};
use std::net::SocketAddr;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct SumidaTestState;

pub struct SumidaTestServer {
  pub client: reqwest::Client,
  addr: SocketAddr,
  shutdown_tx: Option<oneshot::Sender<()>>,
}

impl TestServer {
  pub async fn spawn() -> anyhow::Result<Self> {
    dotenvy::dotenv().ok();

    let settings = infra::settings::get();
    infra::tracing::subscribe();
    infra::db::establish_connection().await;
    infra::aws::load_sdk_config().await;

    let state = SumidaTestState;

    let (messenger_layer, messenger) = infra::messenger::new_layer();
    let app = Router::new()
      .merge(
        api::Sumida::new() //
          .set_state(&state)
          .router(),
      )
      .layer(messenger_layer)
      .layer(api::layer::session::new(infra::session::dynamodb_store::new(&settings.session).await?).await?)
      .layer(api::layer::cors::new())
      .layer(Extension(messenger))
      .layer(Extension(settings))
      .with_state(state)
      .layer(settings.session.ip_source.clone().into_extension());

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

    let client =
      reqwest::Client::builder().cookie_store(true).redirect(reqwest::redirect::Policy::none()).build()?;

    Ok(Self {
      addr,
      client,
      shutdown_tx: Some(shutdown_tx),
    })
  }

  #[inline]
  fn request(&self, method: Method, path: &str) -> RequestBuilder {
    self.client.request(method, self.url(path))
  }

  // Basic HTTP method helpers
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

impl Drop for TestServer {
  fn drop(&mut self) {
    if let Some(tx) = self.shutdown_tx.take() {
      let _ = tx.send(());
    }
  }
}
