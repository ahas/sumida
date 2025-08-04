use std::{net::SocketAddr, time::Duration};

use reqwest::{Method, RequestBuilder, Url};
use tokio::sync::OnceCell;

static CLIENT: OnceCell<TestClient> = OnceCell::const_new();

pub async fn use_client(addr: SocketAddr) -> &'static TestClient {
  CLIENT.get_or_init(|| async { TestClient::new(addr) }).await
}

pub struct TestClient {
  base_url: SocketAddr,
  _client: reqwest::Client,
}

impl TestClient {
  pub fn new(base_url: SocketAddr) -> Self {
    Self {
      base_url,
      _client: reqwest::Client::builder() //
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(30)
        .tcp_keepalive(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/115.0.0.0 Safari/537.36")
        .build()
        .expect("Failed to build reqwest client"),
    }
  }

  // Basic HTTP method helpers
  #[inline]
  pub fn base_url(&self) -> Url {
    Url::parse(&format!("http://{}/", self.base_url)).unwrap()
  }

  #[inline]
  pub fn url(&self, path: &str) -> Url {
    self.base_url().join(path.trim_start_matches('/')).unwrap()
  }

  #[inline]
  fn request(&self, method: Method, path: &str) -> RequestBuilder {
    self._client.request(method, self.url(path))
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
