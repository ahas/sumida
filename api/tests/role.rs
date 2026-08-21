use e2e::prelude::*;

#[tokio::test]
async fn test_role_read_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestRole-{}", random_suffix),
      "description": "Test role for permission testing",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated test role description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_read_permission(&mock, target_role.model.id).await;
}

#[tokio::test]
async fn test_role_create_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let _target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestRole-{}", random_suffix),
      "description": "Test role for permission testing",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated test role description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_create_permission(&mock).await;
}

#[tokio::test]
async fn test_role_update_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestRole-{}", random_suffix),
      "description": "Test role for permission testing",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated test role description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_update_permission(&mock, target_role.model.id).await;
}

#[tokio::test]
async fn test_role_delete_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestRole-{}", random_suffix),
      "description": "Test role for permission testing",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated test role description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_delete_permission(&mock, target_role.model.id).await;
}

#[tokio::test]
async fn test_role_list_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let _target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestRole-{}", random_suffix),
      "description": "Test role for permission testing",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated test role description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_list_permission(&mock).await;
}

#[tokio::test]
async fn test_role_admin_bypass_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestRole-{}", random_suffix),
      "description": "Test role for permission testing",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated test role description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_admin_bypass(&mock, target_role.model.id).await;
}

// 여기서 다루는 정의에는 개별 차단이 없다. 차단은 규칙의 성질이고, 그것을 표현하는 `blocked`
// 관계는 게시판 계열 세 정의(`board_group`/`board`/`board_post`)만 선언한다. 차단할 방법이
// 없는 정의에 차단 테스트를 두면 있는 척하는 테스트가 된다.
// 차단 우선순위 자체는 `api/tests/board.rs::test_board_block_precedence_comprehensive` 가 든다.

#[tokio::test]
async fn test_role_wildcard_action_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_role = TestRole::create(&db).await.expect("Failed to create target role");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("wildcard-action-{}", random_suffix),
      "description": "Wildcard Action Test Role",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated via Wildcard Action"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_wildcard_action_permissions(&mock, target_role.model.id).await;
}

#[tokio::test]
async fn test_role_wildcard_object_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_role1 = TestRole::create(&db).await.expect("Failed to create target role 1");
  let target_role2 = TestRole::create(&db).await.expect("Failed to create target role 2");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("wildcard-object-{}", random_suffix),
      "description": "Wildcard Object Test Role",
      "accessPolicies": []
    }),
    update_data: serde_json::json!({
      "description": "Updated via Wildcard Object"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "role",
    base_url:  "/roles",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite
    .test_wildcard_object_permissions(&mock, target_role1.model.id, target_role2.model.id)
    .await;
}
