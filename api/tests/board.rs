use api::util::random::generate_url_safe_string;
use e2e::prelude::*;
use sea_orm::{ConnectionTrait, EntityTrait, Set};

// Board-specific test helpers
struct TestBoardGroup {
  pub model: entity::bs::board_groups::Model,
}

impl TestBoardGroup {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    use entity::bs::board_groups::{ActiveModel, Entity as BoardGroups};

    let id = BoardGroups::insert(ActiveModel {
      name: Set(format!("TestGroup-{}", generate_url_safe_string(6).unwrap())),
      description: Set(Some(format!("Test Group Description {}", generate_url_safe_string(6).unwrap()))),
      ..Default::default()
    })
    .exec(db)
    .await?
    .last_insert_id;

    Ok(Self {
      model: BoardGroups::find_by_id(id).one(db).await?.unwrap(),
    })
  }
}

struct TestBoard {
  pub model: entity::bs::boards::Model,
}

impl TestBoard {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    use entity::bs::boards::{ActiveModel, Entity as Boards};

    // Create a group first if needed
    let group = TestBoardGroup::create(db).await?;

    let id = Boards::insert(ActiveModel {
      group_id: Set(Some(group.model.id)),
      name: Set(format!("TestBoard-{}", generate_url_safe_string(6).unwrap())),
      description: Set(Some(format!("Test Board Description {}", generate_url_safe_string(6).unwrap()))),
      ..Default::default()
    })
    .exec(db)
    .await?
    .last_insert_id;

    Ok(Self {
      model: Boards::find_by_id(id).one(db).await?.unwrap(),
    })
  }
}

#[tokio::test]
async fn test_board_read_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_read_permission(&mock, target_board.model.id).await;
}

#[tokio::test]
async fn test_board_create_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let _target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_create_permission(&mock).await;
}

#[tokio::test]
async fn test_board_update_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_update_permission(&mock, target_board.model.id).await;
}

#[tokio::test]
async fn test_board_delete_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_delete_permission(&mock, target_board.model.id).await;
}

#[tokio::test]
async fn test_board_list_permissions_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let _target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_list_permission(&mock).await;
}

#[tokio::test]
async fn test_board_admin_bypass_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_admin_bypass(&mock, target_board.model.id).await;
}

#[tokio::test]
async fn test_board_explicit_deny_comprehensive() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("TestBoard-{}", random_suffix),
      "description": "Test board for permission testing"
    }),
    update_data: serde_json::json!({
      "description": "Updated test board description"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_explicit_deny_precedence(&mock, target_board.model.id).await;
}

#[tokio::test]
async fn test_board_wildcard_action_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board = TestBoard::create(&db).await.expect("Failed to create target board");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("wildcard-action-{}", random_suffix),
      "description": "Wildcard Action Test Board"
    }),
    update_data: serde_json::json!({
      "description": "Updated via Wildcard Action"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite.test_wildcard_action_permissions(&mock, target_board.model.id).await;
}

#[tokio::test]
async fn test_board_wildcard_object_permissions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let target_board1 = TestBoard::create(&db).await.expect("Failed to create target board 1");
  let target_board2 = TestBoard::create(&db).await.expect("Failed to create target board 2");

  let random_suffix = api::util::random::generate_url_safe_string(8).unwrap();
  let group = TestBoardGroup::create(&db).await.expect("Failed to create board group");
  let crud_data = CrudTestData {
    create_data: serde_json::json!({
      "groupId": group.model.id,
      "name": format!("wildcard-object-{}", random_suffix),
      "description": "Wildcard Object Test Board"
    }),
    update_data: serde_json::json!({
      "description": "Updated via Wildcard Object"
    }),
  };

  let config = PermissionTestConfig {
    namespace: "board",
    base_url: "/boards",
  };

  let test_suite = PermissionTestSuite::new(config, crud_data);
  test_suite
    .test_wildcard_object_permissions(&mock, target_board1.model.id, target_board2.model.id)
    .await;
}
