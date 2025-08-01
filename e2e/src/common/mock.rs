use sea_orm::DatabaseConnection;

use crate::common::prelude::*;

pub struct Mock<'a> {
  pub client: TestClient<'a>,
  pub db: &'static DatabaseConnection,
  pub server: &'static TestServer,
}

impl<'a> Mock<'a> {
  pub async fn new() -> Self {
    bootstrap::init_once().await;

    let server = use_server().await;

    Self {
      client: TestClient::new(server),
      db: infra::db::get_connection(),
      server,
    }
  }
}
