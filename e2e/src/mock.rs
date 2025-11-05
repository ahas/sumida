use crate::prelude::*;

pub struct Mock {
  pub server: TestServer,
  pub client: TestClient,
}

impl Mock {
  pub async fn new() -> Self {
    bootstrap::init_once().await;

    let server = TestServer::spawn().await.expect("Failed to spawn a server");
    let client = TestClient::new(server.addr);

    Self { server, client }
  }
}

// impl Drop for Mock {
//   fn drop(&mut self) {
//     let _ = self.server.stop();
//   }
// }
