use api::service::prelude::*;
use api::util::ap::{ApUser, EditAp};
use api::util::pagination::List;
use e2e::prelude::*;
use sea_orm::DatabaseConnection;

/// Generalized permission testing utilities for reuse across all controllers
mod permission_test_utils {
  use super::*;
  use reqwest::StatusCode;

  /// Test configuration for permission testing
  pub struct PermissionTestConfig {
    pub namespace: &'static str,
    pub base_url: &'static str,
  }

  /// Helper to create access policies
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

  /// Grant both service and resource level access
  pub async fn grant_service_and_resource_access(
    db: &DatabaseConnection,
    user: &TestUser,
    namespace: &str,
    actions: Vec<String>,
    target_resource_id: Option<i32>,
  ) -> anyhow::Result<()> {
    // Grant service-level access
    create_access_policy(db, user, namespace, "service", actions.clone(), "allow").await?;

    // Grant resource-level access
    let object = target_resource_id.map(|id| id.to_string()).unwrap_or("*".to_string());
    create_access_policy(db, user, namespace, &object, actions, "allow").await?;

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
    create_access_policy(db, user, namespace, "service", vec!["*".to_string()], "allow").await?;

    // Grant resource-level access with wildcard actions
    let object = target_resource_id.map(|id| id.to_string()).unwrap_or("*".to_string());
    create_access_policy(db, user, namespace, &object, vec!["*".to_string()], "allow").await?;

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
    create_access_policy(db, user, namespace, "service", actions.clone(), "allow").await?;

    // Grant wildcard object access (all resources)
    create_access_policy(db, user, namespace, "*", actions, "allow").await?;

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

      grant_service_and_resource_access(
        &db,
        &user,
        self.config.namespace,
        vec!["create".to_string()],
        None,
      )
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

      grant_service_and_resource_access(
        &db,
        &user,
        self.config.namespace,
        vec!["update".to_string()],
        None,
      )
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

      grant_service_and_resource_access(
        &db,
        &user,
        self.config.namespace,
        vec!["delete".to_string()],
        None,
      )
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

      tracing::info!("🔧 Created admin user with ID: {}, is_admin: {}", admin_user.model.id, admin_user.model.is_admin);

      // Check if admin policies exist in DB using SeaORM
      use entity::prelude::*;
      use sea_orm::{EntityTrait, ColumnTrait, QueryFilter};
      let admin_policies = AccessPolicies::find()
        .filter(entity::access_policies::Column::User.eq("user:admin"))
        .all(&db)
        .await
        .expect("Failed to query admin policies");
      tracing::info!("🔍 Found {} admin policies in DB", admin_policies.len());
      for policy in &admin_policies {
        tracing::info!("📋 Admin policy: namespace={}, object={}, actions={:?}, effect={}", 
          policy.namespace, policy.object, policy.action, policy.effect);
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
      assert_eq!(resp.status(), StatusCode::OK, "Admin should be able to read without explicit permission");

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
      assert_eq!(resp.status(), StatusCode::OK, "Admin should be able to list without explicit permission");
    }

    /// Test explicit deny takes precedence over allow
    pub async fn test_explicit_deny_precedence(&self, mock: &Mock, resource_id: i32) {
      let db = infra::db::connect().await;
      let user = TestUser::create(&db).await.expect("Failed to create test user");

      // Grant permission first
      grant_service_and_resource_access(
        &db,
        &user,
        self.config.namespace,
        vec!["delete".to_string()],
        None,
      )
      .await
      .expect("Failed to grant delete permission");

      // Then explicitly deny for specific resource
      create_access_policy(
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
    pub async fn test_wildcard_object_permissions(&self, mock: &Mock, resource_id: i32, other_resource_id: i32) {
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
}

// User Controller specific tests
#[tokio::test] 
async fn test_user_read_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");
  
  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123", 
      "fullName": "New User",
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);
  test_suite.test_read_permission(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_create_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");
  
  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "New User", 
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);
  test_suite.test_create_permission(&mock).await;
}

#[tokio::test]
async fn test_user_update_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");
  
  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "New User",
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);
  test_suite.test_update_permission(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_delete_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");
  
  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "New User",
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);
  test_suite.test_delete_permission(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_list_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");
  
  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "New User",
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);
  test_suite.test_list_permission(&mock).await;
}

#[tokio::test]
async fn test_user_explicit_deny_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");
  
  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "New User",
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);
  test_suite.test_explicit_deny_precedence(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_admin_bypass_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  // Create test data
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = permission_test_utils::CrudTestData {
    create_data: serde_json::json!({
      "username": format!("newuser-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "New User",
      "organizationId": target_user.model.organization_id,
      "roleId": target_user.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated Name"
    }),
  };

  let config = permission_test_utils::PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = permission_test_utils::PermissionTestSuite::new(config, crud_data);

  // Test admin bypass functionality in isolation
  test_suite.test_admin_bypass(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_create_permission_debug() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  
  let user = TestUser::create(&db).await.expect("Failed to create test user");
  
  // Grant create permission
  permission_test_utils::grant_service_and_resource_access(
    &db,
    &user,
    "user",
    vec!["create".to_string()],
    None,
  )
  .await
  .expect("Failed to grant create permission");

  let create_data = serde_json::json!({
    "username": "createtest@example.com",
    "password": "password123",
    "fullName": "Create Test User",
    "organizationId": user.model.organization_id,
    "roleId": user.model.role_id
  });

  tracing::info!("🧪 Testing CREATE with permission");
  user.login(&mock.client).await;
  
  let resp = mock
    .client
    .post("/users")
    .json(&create_data)
    .send()
    .await
    .expect("Failed to send POST request");
    
  let status = resp.status();
  tracing::info!("📝 CREATE response status: {}", status);
  
  if status.is_server_error() {
    let error_text = resp.text().await.expect("Failed to get error text");
    tracing::error!("💥 Server error response: {}", error_text);
  } else if status.is_success() {
    tracing::info!("✅ CREATE succeeded");
  } else {
    let error_text = resp.text().await.expect("Failed to get error text");
    tracing::warn!("⚠️ CREATE failed with status {}: {}", status, error_text);
  }
  
  assert_eq!(status, reqwest::StatusCode::CREATED, "Should be able to create with permission");
}

// Specific wildcard tests for user controller
#[tokio::test]
async fn test_user_wildcard_action_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Grant wildcard action permission (action: ['*'])
  permission_test_utils::grant_wildcard_action_permission(&db, &user, "user", Some(target_user.model.id))
    .await
    .expect("Failed to grant wildcard action permission");

  let login_resp = user.login(&mock.client).await;
  assert_eq!(login_resp.status(), reqwest::StatusCode::OK, "User login should succeed");

  // Test that user can perform all CRUD operations on the specific target user
  let resp = mock
    .client
    .get(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send GET request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard action should allow read");

  let new_user_data = serde_json::json!({
    "username": "wildcard-test@example.com",
    "password": "password123",
    "fullName": "Wildcard Test User",
    "organizationId": user.model.organization_id,
    "roleId": user.model.role_id
  });

  let resp =
    mock.client.post("/users").json(&new_user_data).send().await.expect("Failed to send POST request");
  assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "Wildcard action should allow create");

  let update_data = serde_json::json!({
    "fullName": "Updated via Wildcard"
  });

  let resp = mock
    .client
    .patch(&format!("/users/{}", target_user.model.id))
    .json(&update_data)
    .send()
    .await
    .expect("Failed to send PATCH request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard action should allow update");
}

#[tokio::test]
async fn test_user_wildcard_object_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user1 = TestUser::create(&db).await.expect("Failed to create target user 1");
  let target_user2 = TestUser::create(&db).await.expect("Failed to create target user 2");

  // Grant wildcard object permission for read action only (object: '*')
  permission_test_utils::grant_wildcard_object_permission(&db, &user, "user", vec!["read".to_string()])
    .await
    .expect("Failed to grant wildcard object permission");

  let login_resp = user.login(&mock.client).await;
  assert_eq!(login_resp.status(), reqwest::StatusCode::OK, "User login should succeed");

  // Test that user can read any user with wildcard object permission
  let resp = mock
    .client
    .get(&format!("/users/{}", target_user1.model.id))
    .send()
    .await
    .expect("Failed to send GET request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard object should allow read on any user");

  let resp = mock
    .client
    .get(&format!("/users/{}", target_user2.model.id))
    .send()
    .await
    .expect("Failed to send GET request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard object should allow read on any other user");

  // Test that other actions are still forbidden (only read was granted)
  let update_data = serde_json::json!({
    "fullName": "Should Not Update"
  });

  let resp = mock
    .client
    .patch(&format!("/users/{}", target_user1.model.id))
    .json(&update_data)
    .send()
    .await
    .expect("Failed to send PATCH request");
  assert_eq!(
    resp.status(),
    reqwest::StatusCode::FORBIDDEN,
    "Wildcard object should not allow update without update permission"
  );
}

#[tokio::test]
async fn test_user_list_filtered_by_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let allowed_user = TestUser::create(&db).await.expect("Failed to create allowed user");
  let denied_user = TestUser::create(&db).await.expect("Failed to create denied user");

  // Grant read permission for service and specific user only
  permission_test_utils::create_access_policy(
    &db,
    &user,
    "user",
    "service",
    vec!["read".to_string()],
    "allow",
  )
  .await
  .expect("Failed to create service access policy");
  permission_test_utils::create_access_policy(
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

// Legacy tests for backwards compatibility (can be removed once all tests are migrated)
#[tokio::test]
async fn test_user_read_with_permission() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  // Grant read permission for both service and resource levels
  permission_test_utils::grant_service_and_resource_access(
    &db,
    &user,
    "user",
    vec!["read".to_string()],
    None,
  )
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
async fn test_admin_user_bypasses_restrictions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let admin_user = TestUser::create_admin(&db).await.expect("Failed to create admin user");
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  tracing::info!("🧪 Created admin user with ID: {}, is_admin: {}", admin_user.model.id, admin_user.model.is_admin);
  tracing::info!("🎯 Target user ID: {}", target_user.model.id);

  // Check if admin policies exist in DB using SeaORM
  use entity::prelude::*;
  use sea_orm::{EntityTrait, ColumnTrait, QueryFilter};
  let admin_policies = AccessPolicies::find()
    .filter(entity::access_policies::Column::User.eq("user:admin"))
    .all(&db)
    .await
    .expect("Failed to query admin policies");
  tracing::info!("🔍 Found {} admin policies in DB", admin_policies.len());
  for policy in &admin_policies {
    tracing::info!("📋 Admin policy: namespace={}, object={}, actions={:?}, effect={}", 
      policy.namespace, policy.object, policy.action, policy.effect);
  }

  // Login as admin
  let login_resp = admin_user.login(&mock.client).await;
  tracing::info!("🔑 Admin login response status: {}", login_resp.status());
  assert_eq!(login_resp.status(), reqwest::StatusCode::OK, "Admin login should succeed");

  // Check /me endpoint
  let me_resp = mock.client.get("/me").send().await.expect("Failed to check login status");
  let me_status = me_resp.status();
  tracing::info!("👤 /me endpoint response status: {}", me_status);
  
  if me_status == reqwest::StatusCode::OK {
    let me_data: serde_json::Value = me_resp.json().await.expect("Failed to parse /me response");
    tracing::info!("👤 /me response data: {}", me_data);
  }
  
  assert_eq!(me_status, reqwest::StatusCode::OK, "/me endpoint should work for admin");

  // Should be able to read any user as admin
  tracing::info!("🧪 Testing admin READ on target user ID: {}", target_user.model.id);
  let resp = mock
    .client
    .get(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send GET request");

  tracing::info!("📖 Admin READ response status: {}", resp.status());
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Admin should bypass permissions");
}
