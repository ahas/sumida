use sea_orm::DatabaseConnection;

use crate::prelude::*;

pub struct Mock {
  pub db: &'static DatabaseConnection,
  pub server: TestServer,
  pub client: TestClient,
}

impl Mock {
  pub async fn new() -> Self {
    bootstrap::init_once().await;

    let db = infra::db::get_connection();
    let server = TestServer::spawn().await.expect("Failed to spawn a server");
    let client = TestClient::new(server.addr.clone());

    Self { db, server, client }
  }
}

impl Drop for Mock {
  fn drop(&mut self) {
    let _ = self.server.stop();
  }
}
