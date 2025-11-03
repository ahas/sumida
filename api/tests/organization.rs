use e2e::prelude::*;

#[tokio::test]
async fn test_organization_read_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_read_permission(&mock, target_org.model.id).await;
}

#[tokio::test]
async fn test_organization_create_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let _target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_create_permission(&mock).await;
}

#[tokio::test]
async fn test_organization_update_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_update_permission(&mock, target_org.model.id).await;
}

#[tokio::test]
async fn test_organization_delete_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_delete_permission(&mock, target_org.model.id).await;
}

#[tokio::test]
async fn test_organization_list_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let _target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_list_permission(&mock).await;
}

#[tokio::test]
async fn test_organization_admin_bypass_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_admin_bypass(&mock, target_org.model.id).await;
}

#[tokio::test]
async fn test_organization_explicit_deny_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("TestOrg-{}", random_suffix),
      "description": "Test organization for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test organization description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_explicit_deny_precedence(&mock, target_org.model.id).await;
}

#[tokio::test]
async fn test_organization_wildcard_action_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org = TestOrg::create(&db).await.expect("Failed to create target organization");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("wildcard-action-{}", random_suffix),
      "description": "Wildcard Action Test Organization"
    }),
    update_data: serde_json::json!({
      "description": "Updated via Wildcard Action"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_wildcard_action_permissions(&mock, target_org.model.id).await;
}

#[tokio::test]
async fn test_organization_wildcard_object_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_org1 = TestOrg::create(&db).await.expect("Failed to create target organization 1");
  let target_org2 = TestOrg::create(&db).await.expect("Failed to create target organization 2");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "name": format!("wildcard-object-{}", random_suffix),
      "description": "Wildcard Object Test Organization"
    }),
    update_data: serde_json::json!({
      "description": "Updated via Wildcard Object"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "organization",
    base_url: "/organizations",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite
    .test_wildcard_object_permissions(&mock, target_org1.model.id, target_org2.model.id)
    .await;
}
