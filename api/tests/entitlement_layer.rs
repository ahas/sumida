//! `entitlement_guard!` route-level layer.
//!
//! The entitlement *decision* (subscription-covered vs credit-consume vs denied)
//! is exercised end-to-end through `/entitlements/check` in `membership.rs`; the
//! layer is a thin adapter over `EntitlementService::require_action`. This test
//! pins the layer's fail-closed plumbing: a request that reaches a guarded route
//! without a resolvable session is rejected before the handler runs.

use std::net::SocketAddr;

use api::SumidaState;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use e2e::prelude::*;

#[derive(Clone)]
struct St {
  db: sea_orm::DatabaseConnection,
}
impl SumidaState for St {
  fn db(&self) -> &sea_orm::DatabaseConnection {
    &self.db
  }
}

#[tokio::test]
async fn test_entitlement_guard_fails_closed_without_session() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let state = St { db };

  // A guarded route whose handler would return 200 if it were ever reached.
  let app = Router::new()
    .route("/guarded", get(|| async { StatusCode::OK }))
    .route_layer(api::entitlement_guard!(state, "premium-export"))
    .with_state(state.clone());

  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr: SocketAddr = listener.local_addr().unwrap();
  tokio::spawn(async move {
    axum::serve(listener, app.into_make_service()).await.unwrap();
  });

  // No session layer / no credentials → the guard rejects before the handler.
  let client = reqwest::Client::new();
  let resp = client.get(format!("http://{addr}/guarded")).send().await.unwrap();
  assert_eq!(
    resp.status(),
    reqwest::StatusCode::UNAUTHORIZED,
    "guarded route must fail closed without a session"
  );
}
