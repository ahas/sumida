use api::service::prelude::*;
use e2e::prelude::*;

#[tokio::test(flavor = "multi_thread")]
async fn it_should_be_logged_in() {
  let Mock { ref db, ref client, .. } = Mock::new().await;

  let user = TestUser::create(db).await.expect("Failed to create a test user");
  let resp = user.login(client).await;
  assert!(resp.status().is_success());
}

#[tokio::test(flavor = "multi_thread")]
async fn it_returns_profile_of_authenticated_user() {
  let Mock { ref db, ref client, .. } = Mock::new().await;
  let user = TestUser::create(db).await.expect("Failed to create a test user");

  // 1. Login to the test user
  user.login(client).await;

  // 2. Fetch the user
  let path = &format!("/users/{}", user.model.id);
  let resp = client.get(path).send().await.expect(&format!("Failed to fetch from {path}"));
  assert_eq!(resp.status(), reqwest::StatusCode::OK);

  // 3. Compare the user data with current session
  let json = resp.json::<serde_json::Value>().await.expect("Failed to parse the user item into JSON");
  let item: GetUser = serde_json::from_value(json).expect("Failed to parse the json into GetUser");

  assert_eq!(user.model.id, item.id);
  assert_eq!(user.model.username, item.username);
  assert_eq!(user.model.email, item.email);
  assert_eq!(user.model.full_name, item.full_name);
  assert_eq!(user.model.phone, item.phone);
  assert_eq!(user.model.is_admin, item.is_admin);
  assert_eq!(user.model.is_enabled, item.is_enabled);
  assert_eq!(user.model.role_id, item.role.id);
  assert_eq!(user.model.organization_id, item.organization.id);
  assert_eq!(user.model.created_at, item.created_at);
  assert_eq!(user.model.updated_at, item.updated_at);
}
