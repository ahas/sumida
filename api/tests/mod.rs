pub mod user;

pub use e2e::prelude::*;

#[tokio::test(flavor = "multi_thread")]
async fn health_should_return_ok() -> anyhow::Result<()> {
  let Mock { client, .. } = Mock::new().await;

  let resp = client.get("/health").send().await?;
  assert!(resp.status().is_success());

  let body = resp.text().await?;
  assert_eq!(body, "ok");

  Ok(())
}
