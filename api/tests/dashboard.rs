use e2e::prelude::*;
use futures::StreamExt;
use reqwest::StatusCode;

#[tokio::test]
async fn dashboard_snapshot_requires_admin() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let user = TestUser::create(&db).await.expect("failed to create user");

  let login = user.login(&mock.client).await;
  assert_eq!(login.status(), StatusCode::OK, "user login should succeed");

  let resp = mock.client.get("/dashboard/monitoring").send().await.unwrap();
  assert_eq!(
    resp.status(),
    StatusCode::UNAUTHORIZED,
    "non-admin should not access dashboard monitoring"
  );
}

#[tokio::test]
async fn dashboard_snapshot_returns_components_for_admin() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let admin = TestUser::create_admin(&db).await.expect("failed to create admin user");

  let login = admin.login(&mock.client).await;
  assert_eq!(login.status(), StatusCode::OK, "admin login should succeed");

  let resp = mock.client.get("/dashboard/monitoring").send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "admin should access dashboard");

  let body: serde_json::Value = resp.json().await.expect("invalid json");

  let items = body
    .get("components")
    .and_then(|v| v.get("items"))
    .and_then(|v| v.as_array())
    .cloned()
    .unwrap_or_default();
  assert!(!items.is_empty(), "components should be listed");

  let memory_total = body
    .get("snapshot")
    .and_then(|s| s.get("resources"))
    .and_then(|r| r.get("memoryTotalBytes"))
    .and_then(|v| v.as_u64());
  assert!(memory_total.is_some(), "should include memory totals");
}

#[tokio::test]
async fn dashboard_stream_requires_admin() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let user = TestUser::create(&db).await.expect("failed to create user");

  let login = user.login(&mock.client).await;
  assert_eq!(login.status(), StatusCode::OK, "user login should succeed");

  let resp = mock.client.get("/dashboard/monitoring/stream").send().await.unwrap();
  assert_eq!(
    resp.status(),
    StatusCode::UNAUTHORIZED,
    "non-admin should not subscribe to dashboard stream"
  );
}

#[tokio::test]
async fn dashboard_stream_returns_event_stream_for_admin() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let admin = TestUser::create_admin(&db).await.expect("failed to create admin user");

  let login = admin.login(&mock.client).await;
  assert_eq!(login.status(), StatusCode::OK, "admin login should succeed");

  let resp = mock.client.get("/dashboard/monitoring/stream").send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "admin should access stream");

  let content_type = resp
    .headers()
    .get(reqwest::header::CONTENT_TYPE)
    .and_then(|v| v.to_str().ok())
    .unwrap_or("");
  assert!(
    content_type.contains("text/event-stream"),
    "stream should expose event-stream content type"
  );

  let mut stream = resp.bytes_stream();
  let first_chunk = stream.next().await;
  assert!(first_chunk.is_some(), "stream should yield data");
}
