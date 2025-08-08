pub mod user;

pub use e2e::prelude::*;

#[tokio::test]
async fn health_should_return_ok() {
  let Mock { ref client, .. } = Mock::new().await;

  let resp = client.get("/health").send().await.expect("Failed to fetch GET from /health");
  assert!(resp.status().is_success());

  let body = resp.text().await.expect("Failed to read text from response");
  assert_eq!(body, "ok");
}
