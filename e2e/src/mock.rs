use crate::prelude::*;

pub struct Mock {
  pub server: TestServer,
  pub client: TestClient,
}

impl Mock {
  /// A server carrying sumida's rules and nothing else.
  pub async fn new() -> Self {
    Self::with_rules(&[]).await
  }

  /// A server carrying `rules` as well.
  ///
  /// Pass whatever the test binary installed. The router registers what it is given, so a binary
  /// that installed a rule file and then built a router declaring none would be two declarations
  /// disagreeing — and `install` refuses that rather than picking one.
  pub async fn with_rules(rules: &'static [api::authz::registry::RuleFile]) -> Self {
    bootstrap::init_once().await;

    let server = TestServer::spawn_with_rules(rules).await.expect("Failed to spawn a server");
    let client = TestClient::new(server.addr);

    Self { server, client }
  }
}

// impl Drop for Mock {
//   fn drop(&mut self) {
//     let _ = self.server.stop();
//   }
// }
