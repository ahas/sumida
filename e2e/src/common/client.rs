use std::time::Duration;

use reqwest::{Method, RequestBuilder, Url};

use crate::common::server::TestServer;

pub struct TestClient<'a> {
  _server: &'a TestServer,
  _client: reqwest::Client,
}

impl<'a> TestClient<'a> {
  pub fn new(server: &'a TestServer) -> Self {
    Self {
      _server: server,
      _client: reqwest::Client::builder() //
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .expect("Failed to build reqwest client"),
    }
  }

  // Basic HTTP method helpers
  #[inline]
  pub fn base_url(&self) -> Url {
    Url::parse(&format!("http://{}/", self._server.addr)).unwrap()
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
