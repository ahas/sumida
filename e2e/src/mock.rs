use sea_orm::DatabaseConnection;

use crate::prelude::*;

pub struct Mock {
  pub db: DatabaseConnection,
  pub server: TestServer,
  pub client: TestClient,
}

impl Mock {
  pub async fn new() -> Self {
    bootstrap::init_once().await;

    // let db = infra::db::establish_connection().await;
    // let server = use_server().await;
    // let client = use_client(server).await;
    let db = infra::db::connect().await;
    let server = TestServer::spawn().await.expect("Failed to spawn a server");
    let client = TestClient::new(server.addr.clone());

    Self { db, server, client }
  }
}
