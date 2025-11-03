use api::util::ap::{ApUser, EditAp};
use reqwest::StatusCode;
use sea_orm::DatabaseConnection;

use crate::prelude::*;

/// Test configuration for permission testing
pub struct PermissionTestConfig {
  pub namespace: &'static str,
  pub base_url: &'static str,
}

/// Helper to create access policies
pub async fn create_ap(
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

/// Grant both service and resource level access
pub async fn grant_service_and_resource_access(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  actions: Vec<String>,
  target_resource_id: Option<i32>,
) -> anyhow::Result<()> {
  // Grant service-level access
  create_ap(db, user, namespace, "service", actions.clone(), "allow").await?;

  // Grant resource-level access
  let object = target_resource_id.map(|id| id.to_string()).unwrap_or("*".to_string());
  create_ap(db, user, namespace, &object, actions, "allow").await?;

  Ok(())
}

/// Grant wildcard action permissions (all actions allowed)
pub async fn grant_wildcard_action_permission(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  target_resource_id: Option<i32>,
) -> anyhow::Result<()> {
  // Grant service-level access with wildcard actions
  create_ap(db, user, namespace, "service", vec!["*".to_string()], "allow").await?;

  // Grant resource-level access with wildcard actions
  let object = target_resource_id.map(|id| id.to_string()).unwrap_or("*".to_string());
  create_ap(db, user, namespace, &object, vec!["*".to_string()], "allow").await?;

  Ok(())
}

/// Grant wildcard object permissions (all objects allowed)
pub async fn grant_wildcard_object_permission(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  actions: Vec<String>,
) -> anyhow::Result<()> {
  // Grant service-level access
  create_ap(db, user, namespace, "service", actions.clone(), "allow").await?;

  // Grant wildcard object access (all resources)
  create_ap(db, user, namespace, "*", actions, "allow").await?;

  Ok(())
}

/// Test structure for CRUD operations
pub struct CrudTestData {
  pub create_data: serde_json::Value,
  pub update_data: serde_json::Value,
}

/// Comprehensive permission test suite for any controller
pub struct PermissionTestSuite {
  pub config: PermissionTestConfig,
  pub crud_data: CrudTestData,
}

impl PermissionTestSuite {
  pub fn new(config: PermissionTestConfig, crud_data: CrudTestData) -> Self {
    Self { config, crud_data }
  }

  /// Test READ action with and without permission
  pub async fn test_read_permission(&self, mock: &Mock, resource_id: i32) {
    let db = infra::db::connect().await;

    // Test with permission
    let user = TestUser::create(&db).await.expect("Failed to create test user");
    grant_service_and_resource_access(&db, &user, self.config.namespace, vec!["read".to_string()], None)
      .await
      .expect("Failed to grant read permission");

    user.login(&mock.client).await;
    let resp = mock
      .client
      .get(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::OK, "Should be able to read with permission");

    // Test without permission - create new mock for isolation
    let mock2 = Mock::new().await;
    let user2 = TestUser::create(&db).await.expect("Failed to create test user");
    user2.login(&mock2.client).await;
    let resp = mock2
      .client
      .get(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Should be forbidden without permission");
  }

  /// Test CREATE action with and without permission
  pub async fn test_create_permission(&self, mock: &Mock) {
    // Test with permission
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    grant_service_and_resource_access(&db, &user, self.config.namespace, vec!["create".to_string()], None)
      .await
      .expect("Failed to grant create permission");

    user.login(&mock.client).await;
    let resp = mock
      .client
      .post(self.config.base_url)
      .json(&self.crud_data.create_data)
      .send()
      .await
      .expect("Failed to send POST request");
    assert_eq!(resp.status(), StatusCode::CREATED, "Should be able to create with permission");

    // Test without permission
    let mock2 = Mock::new().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");
    user.login(&mock2.client).await;
    let resp = mock2
      .client
      .post(self.config.base_url)
      .json(&self.crud_data.create_data)
      .send()
      .await
      .expect("Failed to send POST request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Should be forbidden without permission");
  }

  /// Test UPDATE action with and without permission
  pub async fn test_update_permission(&self, mock: &Mock, resource_id: i32) {
    // Test with permission
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    grant_service_and_resource_access(&db, &user, self.config.namespace, vec!["update".to_string()], None)
      .await
      .expect("Failed to grant update permission");

    user.login(&mock.client).await;
    let resp = mock
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(resp.status(), StatusCode::OK, "Should be able to update with permission");

    // Test without permission
    let mock2 = Mock::new().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");
    user.login(&mock2.client).await;
    let resp = mock2
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Should be forbidden without permission");
  }

  /// Test DELETE action with and without permission
  pub async fn test_delete_permission(&self, mock: &Mock, resource_id: i32) {
    // Test with permission
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    grant_service_and_resource_access(&db, &user, self.config.namespace, vec!["delete".to_string()], None)
      .await
      .expect("Failed to grant delete permission");

    user.login(&mock.client).await;
    let resp = mock
      .client
      .delete(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send DELETE request");
    assert_eq!(resp.status(), StatusCode::OK, "Should be able to delete with permission");

    // Test without permission
    let mock2 = Mock::new().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");
    user.login(&mock2.client).await;
    let resp = mock2
      .client
      .delete(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send DELETE request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Should be forbidden without permission");
  }

  /// Test LIST action with and without permission
  pub async fn test_list_permission(&self, mock: &Mock) {
    // Test with permission
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    grant_service_and_resource_access(&db, &user, self.config.namespace, vec!["read".to_string()], None)
      .await
      .expect("Failed to grant read permission");

    user.login(&mock.client).await;
    let resp = mock.client.get(self.config.base_url).send().await.expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::OK, "Should be able to list with permission");

    // Test without permission
    let mock2 = Mock::new().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");
    user.login(&mock2.client).await;
    let resp = mock2.client.get(self.config.base_url).send().await.expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Should be forbidden without permission");
  }

  /// Test admin bypass functionality
  pub async fn test_admin_bypass(&self, mock: &Mock, resource_id: i32) {
    let db = infra::db::connect().await;
    let admin_user = TestUser::create_admin(&db).await.expect("Failed to create admin user");

    tracing::info!(
      "🔧 Created admin user with ID: {}, is_admin: {}",
      admin_user.model.id,
      admin_user.model.is_admin
    );

    // Check if admin policies exist in DB using SeaORM
    use entity::prelude::*;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let admin_policies = AccessPolicies::find()
      .filter(entity::access_policies::Column::User.eq("user:admin"))
      .all(&db)
      .await
      .expect("Failed to query admin policies");
    tracing::info!("🔍 Found {} admin policies in DB", admin_policies.len());
    for policy in &admin_policies {
      tracing::info!(
        "📋 Admin policy: namespace={}, object={}, actions={:?}, effect={}",
        policy.namespace,
        policy.object,
        policy.action,
        policy.effect
      );
    }

    // Do not grant any explicit permissions - admin should bypass
    let login_resp = admin_user.login(&mock.client).await;
    tracing::info!("🔑 Admin login response status: {}", login_resp.status());
    assert_eq!(login_resp.status(), StatusCode::OK, "Admin login should succeed");

    // Verify admin is logged in by checking /me endpoint
    let me_resp = mock.client.get("/me").send().await.expect("Failed to check login status");
    let me_status = me_resp.status();
    tracing::info!("👤 /me endpoint response status: {}", me_status);

    if me_status == StatusCode::OK {
      let me_data: serde_json::Value = me_resp.json().await.expect("Failed to parse /me response");
      tracing::info!("👤 /me response data: {}", me_data);
    }

    assert_eq!(me_status, StatusCode::OK, "Admin should be logged in successfully");

    // Test all CRUD operations as admin
    tracing::info!("🧪 Testing admin READ on resource ID: {}", resource_id);
    let resp = mock
      .client
      .get(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send GET request");
    tracing::info!("📖 Admin READ response status: {}", resp.status());
    assert_eq!(
      resp.status(),
      StatusCode::OK,
      "Admin should be able to read without explicit permission"
    );

    let resp = mock
      .client
      .post(self.config.base_url)
      .json(&self.crud_data.create_data)
      .send()
      .await
      .expect("Failed to send POST request");
    assert_eq!(
      resp.status(),
      StatusCode::CREATED,
      "Admin should be able to create without explicit permission"
    );

    let resp = mock
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(
      resp.status(),
      StatusCode::OK,
      "Admin should be able to update without explicit permission"
    );

    let resp = mock.client.get(self.config.base_url).send().await.expect("Failed to send GET request");
    assert_eq!(
      resp.status(),
      StatusCode::OK,
      "Admin should be able to list without explicit permission"
    );
  }

  /// Test explicit deny takes precedence over allow
  pub async fn test_explicit_deny_precedence(&self, mock: &Mock, resource_id: i32) {
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    // Grant permission first
    grant_service_and_resource_access(&db, &user, self.config.namespace, vec!["delete".to_string()], None)
      .await
      .expect("Failed to grant delete permission");

    // Then explicitly deny for specific resource
    create_ap(
      &db,
      &user,
      self.config.namespace,
      &resource_id.to_string(),
      vec!["delete".to_string()],
      "deny",
    )
    .await
    .expect("Failed to create deny policy");

    user.login(&mock.client).await;
    let resp = mock
      .client
      .delete(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send DELETE request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Explicit deny should take precedence over allow");
  }

  /// Test wildcard action permissions (action: ['*'] allows all actions)
  pub async fn test_wildcard_action_permissions(&self, mock: &Mock, resource_id: i32) {
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    // Grant wildcard action permission
    grant_wildcard_action_permission(&db, &user, self.config.namespace, Some(resource_id))
      .await
      .expect("Failed to grant wildcard action permission");

    let login_resp = user.login(&mock.client).await;
    assert_eq!(login_resp.status(), StatusCode::OK, "User login should succeed");

    // Test that all CRUD operations are allowed with wildcard action
    let resp = mock
      .client
      .get(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::OK, "Wildcard action should allow read");

    let resp = mock
      .client
      .post(self.config.base_url)
      .json(&self.crud_data.create_data)
      .send()
      .await
      .expect("Failed to send POST request");
    assert_eq!(resp.status(), StatusCode::CREATED, "Wildcard action should allow create");

    let resp = mock
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(resp.status(), StatusCode::OK, "Wildcard action should allow update");

    let resp = mock.client.get(self.config.base_url).send().await.expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::OK, "Wildcard action should allow list");

    // Note: Testing delete last as it might affect other tests
    let resp = mock
      .client
      .delete(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send DELETE request");
    assert_eq!(resp.status(), StatusCode::OK, "Wildcard action should allow delete");
  }

  /// Test wildcard object permissions (object: '*' allows access to all objects)
  pub async fn test_wildcard_object_permissions(
    &self,
    mock: &Mock,
    resource_id: i32,
    other_resource_id: i32,
  ) {
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    // Grant wildcard object permission for read action only
    grant_wildcard_object_permission(&db, &user, self.config.namespace, vec!["read".to_string()])
      .await
      .expect("Failed to grant wildcard object permission");

    let login_resp = user.login(&mock.client).await;
    assert_eq!(login_resp.status(), StatusCode::OK, "User login should succeed");

    // Test that user can read any object with wildcard object permission
    let resp = mock
      .client
      .get(&format!("{}/{}", self.config.base_url, resource_id))
      .send()
      .await
      .expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::OK, "Wildcard object should allow read on first resource");

    let resp = mock
      .client
      .get(&format!("{}/{}", self.config.base_url, other_resource_id))
      .send()
      .await
      .expect("Failed to send GET request");
    assert_eq!(resp.status(), StatusCode::OK, "Wildcard object should allow read on second resource");

    // Test that other actions are still forbidden (only read was granted)
    let resp = mock
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(
      resp.status(),
      StatusCode::FORBIDDEN,
      "Wildcard object should not allow update without update permission"
    );
  }
}
