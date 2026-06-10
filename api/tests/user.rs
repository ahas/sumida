use api::service::prelude::*;
use api::util::pagination::List;
use e2e::ap_test_utils;
use e2e::prelude::*;

#[tokio::test]
async fn health_should_return_ok() {
  let Mock { ref client, .. } = Mock::new().await;

  let resp = client.get("/health").send().await.expect("Failed to fetch GET from /health");
  assert!(resp.status().is_success());

  let body = resp.text().await.expect("Failed to read text from response");
  assert_eq!(body, "ok");
}

// User Controller specific tests
#[tokio::test]
async fn test_user_read_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
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

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_read_permission(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_create_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
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

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_create_permission(&mock).await;
}

#[tokio::test]
async fn test_user_update_permissions_comprehensive() {
  // User update authorization differs from other resources: a non-admin operator may only
  // update users in their own organization (or descendant orgs) and must hold an `org:user`
  // update policy (see UserService::update). So we put the operator and target in the SAME
  // organization and grant the `org:user` policy, rather than using the generic `user` suite.
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let operator = TestUser::create(&db).await.expect("Failed to create operator");
  // Target user in the same organization as the operator.
  let target_user = TestUser::create_in_org(&db, operator.model.organization_id)
    .await
    .expect("Failed to create target user");

  // update has a two-stage gate:
  //   (1) the `service_ap!("user", update)` route layer → needs a `user` service policy
  //   (2) the in-service org-scoped row guard → needs an `org:user` update policy
  // Grant both.
  ap_test_utils::create_ap(&db, &operator, "user", "service", vec!["update".to_string()], "allow")
    .await
    .expect("Failed to grant user service permission");
  ap_test_utils::grant_service_and_resource_access(
    &db,
    &operator,
    "org:user",
    vec!["update".to_string()],
    None,
  )
  .await
  .expect("Failed to grant org:user update permission");

  operator.login(&mock.client).await;
  let resp = mock
    .client
    .patch(&format!("/users/{}", target_user.model.id))
    .json(&serde_json::json!({ "fullName": "Updated Name" }))
    .send()
    .await
    .expect("Failed to send PATCH request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Should be able to update with org:user permission");

  // Without permission: a fresh operator with no policy is forbidden.
  let mock2 = Mock::new().await;
  let no_perm = TestUser::create(&db).await.expect("Failed to create user");
  no_perm.login(&mock2.client).await;
  let resp = mock2
    .client
    .patch(&format!("/users/{}", target_user.model.id))
    .json(&serde_json::json!({ "fullName": "Updated Name" }))
    .send()
    .await
    .expect("Failed to send PATCH request");
  assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN, "Should be forbidden without permission");
}

#[tokio::test]
async fn test_user_delete_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
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

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_delete_permission(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_list_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
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

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_list_permission(&mock).await;
}

#[tokio::test]
async fn test_user_explicit_deny_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
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

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_explicit_deny_precedence(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_admin_bypass_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  // Create test data
  let target_user = TestUser::create(&db).await.expect("Failed to create target user");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
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

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);

  // Test admin bypass functionality in isolation
  test_suite.test_admin_bypass(&mock, target_user.model.id).await;
}

#[tokio::test]
async fn test_user_create_permission_debug() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("Failed to create test user");

  // Grant create permission
  ap_test_utils::grant_service_and_resource_access(&db, &user, "user", vec!["create".to_string()], None)
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
  // Target in the same org as the operator (user update is org-scoped, see UserService::update).
  let target_user = TestUser::create_in_org(&db, user.model.organization_id)
    .await
    .expect("Failed to create target user");

  // Grant wildcard read on `user` (read/create/list are gated only by the `user` service policy)
  // and wildcard action on `org:user` (update/delete go through the org-scoped row guard).
  ap_test_utils::grant_wildcard_action_permission(&db, &user, "user", Some(target_user.model.id))
    .await
    .expect("Failed to grant wildcard action permission on user");
  // org:user objects are organization ids (matched via the org-ancestors subquery), so use
  // a wildcard object here rather than the target user's id.
  ap_test_utils::grant_wildcard_action_permission(&db, &user, "org:user", None)
    .await
    .expect("Failed to grant wildcard action permission on org:user");

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

  let resp = mock
    .client
    .post("/users")
    .json(&new_user_data)
    .send()
    .await
    .expect("Failed to send POST request");
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
  ap_test_utils::grant_wildcard_object_permission(&db, &user, "user", vec!["read".to_string()])
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
  assert_eq!(
    resp.status(),
    reqwest::StatusCode::OK,
    "Wildcard object should allow read on any other user"
  );

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
  ap_test_utils::create_access_policy(&db, &user, "user", "service", vec!["read".to_string()], "allow")
    .await
    .expect("Failed to create service access policy");
  ap_test_utils::create_access_policy(
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
  ap_test_utils::grant_service_and_resource_access(&db, &user, "user", vec!["read".to_string()], None)
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

  tracing::info!(
    "🧪 Created admin user with ID: {}, is_admin: {}",
    admin_user.model.id,
    admin_user.model.is_admin
  );
  tracing::info!("🎯 Target user ID: {}", target_user.model.id);

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

#[tokio::test]
async fn test_wildcard_action_permissions() {
  // Full-CRUD wildcard test honoring the user resource's split authorization model:
  // read/create/list are gated by the `user` service policy, while update/delete additionally
  // require an org-scoped `org:user` policy and a same-org target.
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let operator = TestUser::create(&db).await.expect("Failed to create operator");
  let target_user = TestUser::create_in_org(&db, operator.model.organization_id)
    .await
    .expect("Failed to create target user");

  ap_test_utils::grant_wildcard_action_permission(&db, &operator, "user", Some(target_user.model.id))
    .await
    .expect("Failed to grant wildcard action on user");
  // org:user objects are organization ids (matched via the org-ancestors subquery), so use
  // a wildcard object here rather than the target user's id.
  ap_test_utils::grant_wildcard_action_permission(&db, &operator, "org:user", None)
    .await
    .expect("Failed to grant wildcard action on org:user");

  operator.login(&mock.client).await;

  let resp = mock
    .client
    .get(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send GET request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard action should allow read");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let resp = mock
    .client
    .post("/users")
    .json(&serde_json::json!({
      "username": format!("wildcard-action-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "Wildcard Action Test User",
      "organizationId": operator.model.organization_id,
      "roleId": operator.model.role_id
    }))
    .send()
    .await
    .expect("Failed to send POST request");
  assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "Wildcard action should allow create");

  let resp = mock
    .client
    .patch(&format!("/users/{}", target_user.model.id))
    .json(&serde_json::json!({ "fullName": "Updated via Wildcard Action" }))
    .send()
    .await
    .expect("Failed to send PATCH request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard action should allow update");

  let resp = mock.client.get("/users").send().await.expect("Failed to send GET request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard action should allow list");

  let resp = mock
    .client
    .delete(&format!("/users/{}", target_user.model.id))
    .send()
    .await
    .expect("Failed to send DELETE request");
  assert_eq!(resp.status(), reqwest::StatusCode::OK, "Wildcard action should allow delete");
}

#[tokio::test]
async fn test_wildcard_object_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_user1 = TestUser::create(&db).await.expect("Failed to create target user 1");
  let target_user2 = TestUser::create(&db).await.expect("Failed to create target user 2");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "username": format!("wildcard-object-{}@example.com", random_suffix),
      "password": "password123",
      "fullName": "Wildcard Object Test User",
      "organizationId": target_user1.model.organization_id,
      "roleId": target_user1.model.role_id
    }),
    update_data: serde_json::json!({
      "fullName": "Updated via Wildcard Object"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "user",
    base_url: "/users",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite
    .test_wildcard_object_permissions(&mock, target_user1.model.id, target_user2.model.id)
    .await;
}
