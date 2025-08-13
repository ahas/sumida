use api::service::prelude::*;
use api::util::ap::{ApUser, EditAp};
use api::util::pagination::List;
use e2e::prelude::*;
use sea_orm::DatabaseConnection;

mod helpers {
  use super::*;

  pub async fn create_access_policy(
    db: &DatabaseConnection,
    user: &TestUser,
    namespace: &str,
    object: &str,
    actions: Vec<String>,
    effect: &str,
  ) -> anyhow::Result<()> {
    EditAp::new()
      .namespace(namespace)
      .object(object)
      .action(actions)
      .user(ApUser::User(user.model.id))
      .create(effect, db)
      .await?;
    Ok(())
  }

  pub async fn grant_service_and_resource_access(
    db: &DatabaseConnection,
    user: &TestUser,
    namespace: &str,
    actions: Vec<String>,
    target_user_id: Option<i32>,
  ) -> anyhow::Result<()> {
    // Grant service-level access
    create_access_policy(db, user, namespace, "service", actions.clone(), "allow").await?;
    
    // Grant resource-level access
    let object = target_user_id.map(|id| id.to_string()).unwrap_or("*".to_string());
    create_access_policy(db, user, namespace, &object, actions, "allow").await?;
    
    Ok(())
  }
}

#[tokio::test]
async fn test_user_read_with_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Grant read permission for both service and resource levels
  helpers::grant_service_and_resource_access(&db, &user, "user", vec!["read".to_string()], None)
    .await
    .expect("Failed to create access policy");

  // Login as the user
  user.login(&mock.client).await;

  // Should be able to read target user
  let resp = mock
    .client
    .get(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send GET request");

  assert_eq!(resp.status(), reqwest::StatusCode::OK);

  let user_data: GetUser = resp.json().await.expect("Failed to parse response");
  assert_eq!(user_data.id, target_user.model.id);
}

#[tokio::test]
async fn test_user_read_without_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Do not grant any permission

  // Login as the user
  user.login(&mock.client).await;

  // Should be forbidden to read target user
  let resp = mock
    .client
    .get(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send GET request");

  assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_user_create_with_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");

  // Grant create permission for both service and resource levels
  helpers::grant_service_and_resource_access(&db, &user, "user", vec!["create".to_string()], None)
    .await
    .expect("Failed to create access policy");

  // Login as the user
  user.login(&mock.client).await;

  let new_user_data = serde_json::json!({
      "username": "newuser@example.com",
      "password": "password123",
      "fullName": "New User",
      "organizationId": user.model.organization_id,
      "roleId": user.model.role_id
  });

  // Should be able to create user
  let resp =
    mock.client.post("/users").json(&new_user_data).send().await.expect("Failed to send POST request");

  assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

  let created_user: UserCreated = resp.json().await.expect("Failed to parse response");
  assert!(created_user.id > 0);
}

#[tokio::test]
async fn test_user_create_without_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");

  // Do not grant create permission

  // Login as the user
  user.login(&mock.client).await;

  let new_user_data = serde_json::json!({
      "username": "newuser@example.com",
      "password": "password123",
      "fullName": "New User",
      "organizationId": user.model.organization_id,
      "roleId": user.model.role_id
  });

  // Should be forbidden to create user
  let resp =
    mock.client.post("/users").json(&new_user_data).send().await.expect("Failed to send POST request");

  assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_user_update_with_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Grant update permission for both service and all resources (since object_ref uses dynamic SQL)
  helpers::grant_service_and_resource_access(&db, &user, "user", vec!["update".to_string()], None)
    .await
    .expect("Failed to create access policy");

  // Login as the user
  user.login(&mock.client).await;

  let update_data = serde_json::json!({
      "fullName": "Updated Name"
  });

  // Should be able to update target user
  let resp = mock
    .client
    .patch(&format!("/users/{}", target_user.model.id))
    .json(&update_data)
    .send()
    .await
    .expect("Failed to send PATCH request");

  assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_user_delete_with_explicit_deny() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Grant delete permission first for all resources
  helpers::grant_service_and_resource_access(&db, &user, "user", vec!["delete".to_string()], None)
    .await
    .expect("Failed to create allow policy");

  // Then explicitly deny for specific resource
  helpers::create_access_policy(
    &db,
    &user,
    "user",
    &target_user.model.id.to_string(),
    vec!["delete".to_string()],
    "deny",
  )
  .await
  .expect("Failed to create deny policy");

  // Login as the user
  user.login(&mock.client).await;

  // Should be forbidden due to explicit deny (deny takes precedence)
  let resp = mock
    .client
    .delete(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send DELETE request");

  assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_user_list_filtered_by_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let allowed_user = TestUser::create(&db).await.expect("Failed to create allowed user");
  let denied_user = TestUser::create(&db).await.expect("Failed to create denied user");

  // Grant read permission for service and specific user only
  helpers::create_access_policy(&db, &user, "user", "service", vec!["read".to_string()], "allow")
    .await
    .expect("Failed to create service access policy");
  helpers::create_access_policy(
    &db,
    &user,
    "user",
    &allowed_user.model.id.to_string(),
    vec!["read".to_string()],
    "allow",
  )
  .await
  .expect("Failed to create resource access policy");

  // Login as the user
  user.login(&mock.client).await;

  // List users - should only see the allowed user (and possibly self)
  let resp = mock.client.get("/users").send().await.expect("Failed to send GET request");

  assert_eq!(resp.status(), reqwest::StatusCode::OK);

  let user_list: List<UserListItem> = resp.json().await.expect("Failed to parse response");

  // Should contain allowed_user but not denied_user
  let response_json: serde_json::Value =
    serde_json::from_str(&serde_json::to_string(&user_list).unwrap()).unwrap();
  let items = response_json["items"].as_array().unwrap();
  let user_ids: Vec<i32> = items.iter().map(|u| u["id"].as_i64().unwrap() as i32).collect();
  assert!(user_ids.contains(&allowed_user.model.id));
  assert!(!user_ids.contains(&denied_user.model.id));
}

#[tokio::test]
async fn test_admin_user_bypasses_restrictions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let admin_user = TestUser::create_admin(&db).await.expect("Failed to create admin user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Do not grant any explicit permissions (admin should have implicit access)

  // Login as admin
  admin_user.login(&mock.client).await;

  // Should be able to read any user as admin
  let resp = mock
    .client
    .get(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send GET request");

  assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

