use api::service::prelude::*;
use e2e::common::prelude::*;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{EntityTrait, Set};

#[tokio::test]
async fn it_should_be_logged_in() -> anyhow::Result<()> {
  let Mock { db, client, .. } = Mock::new().await;
  let user = TestUser::create(db).await.expect("Failed to create a test user");
  let resp = user.login(&client).await;

  assert_eq!(resp.status(), StatusCode::OK);

  Ok(())
}

#[tokio::test]
async fn it_returns_profile_of_authenticated_user() -> anyhow::Result<()> {
  let Mock { client, db, .. } = Mock::new().await;
  let user = TestUser::create(db).await.expect("Failed to create a test user");

  // 1. Clear and create a user role
  AccessPolicies::delete_many().exec(db).await?;
  AccessPolicies::insert(entity::access_policies::ActiveModel {
    namespace: Set("*".to_string()),
    user: Set("user:admin".to_string()),
    object: Set("*".to_string()),
    action: Set(vec!["*".to_string()]),
    effect: Set("allow".to_string()),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("Failed to insert the admin access policy");

  // 2. Login to the test user
  user.login(&client).await;

  // 3. Fetch the user
  let path = &format!("/users/{}", user.model.id);
  let resp = client.get(path).send().await.expect(&format!("Failed to fetch from {path}"));
  assert_eq!(resp.status(), StatusCode::OK);

  // 4. Compare the user data with current session
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

  Ok(())
}
