use e2e::ap_test_utils::grant_service;
use e2e::prelude::*;
use reqwest::StatusCode;

const FILTERS_URL: &str = "/board-posts/filters";

/// The declaration the list actually reads, served to the client that has to build a form from it.
#[tokio::test]
async fn board_post_filters_describe_the_declared_surface() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let user = TestUser::create(&db).await.expect("Failed to create test user");

  grant_service(&db, &user, "board_post", vec!["read".to_string()])
    .await
    .expect("Failed to grant read permission");

  user.login(&mock.client).await;

  let resp = mock.client.get(FILTERS_URL).send().await.expect("Failed to send GET request");
  assert_eq!(resp.status(), StatusCode::OK, "Should be able to read the descriptor with permission");

  let body: serde_json::Value = resp.json().await.expect("the descriptor is JSON");

  assert_eq!(body["defaultSort"], "created_at");
  assert!(body["limits"]["maxLimit"].as_u64().unwrap() > 0);

  let fields = body["fields"].as_array().expect("the descriptor carries a field list");
  let field =
    |key: &str| fields.iter().find(|f| f["key"] == key).unwrap_or_else(|| panic!("`{key}` is declared"));

  // The full-text fields say so, and none of them is offered as a substring match.
  assert_eq!(field("text")["strategy"], "fullText");
  assert_eq!(field("text")["ops"], serde_json::json!(["matches"]));
  assert_eq!(field("search")["strategy"], "custom");

  // An enum carries its variants; a relation-ish key carries where it points.
  assert!(
    field("status")["variants"]
      .as_array()
      .unwrap()
      .contains(&serde_json::json!("published"))
  );
  assert_eq!(field("board_id")["relation"]["resource"], "/boards");

  // No declared key collides with what the list envelope already owns.
  for reserved in ["page", "limit", "sort", "order", "filter", "count"] {
    assert!(
      !fields.iter().any(|f| f["key"] == reserved),
      "`{reserved}` is reserved and may not be a field key"
    );
  }

  // Sortability lives on the same list the filter fields do.
  assert_eq!(field("title")["sortable"], true);
  assert_eq!(field("created_at")["sortable"], true);
}

/// Same gates as the list it describes: a caller who cannot list cannot enumerate the fields.
#[tokio::test]
async fn board_post_filters_sit_behind_the_list_gate() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let user = TestUser::create(&db).await.expect("Failed to create test user");

  user.login(&mock.client).await;

  let resp = mock.client.get(FILTERS_URL).send().await.expect("Failed to send GET request");
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Should be forbidden without the read gate");
}

#[tokio::test]
async fn board_post_filters_need_a_session() {
  let mock = Mock::new().await;

  let resp = mock.client.get(FILTERS_URL).send().await.expect("Failed to send GET request");
  assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "Should be unauthorized without a session");
}
