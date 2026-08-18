use api::util::random::generate_url_safe_string;
use e2e::prelude::*;
use sea_orm::{ConnectionTrait, EntityTrait, Set};

// Board-specific test helpers
struct TestBoardGroup {
  pub model: entity::bps::board_groups::Model,
}

impl TestBoardGroup {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    use entity::bps::board_groups::{ActiveModel, Entity as BoardGroups};

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
  pub model: entity::bps::boards::Model,
}

impl TestBoard {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    use entity::bps::boards::{ActiveModel, Entity as Boards};

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
async fn test_board_block_precedence_comprehensive() {
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
  test_suite.test_block_precedence(&mock, target_board.model.id).await;
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

// `test_board_wildcard_object_permissions` 는 없어졌다. `board#view` 와 `board#edit` 에는
// `service:board->operate_read` / `operate_update` 갈래가 없다 — 게시판에 대한
// "모든 행" 권한은 규칙상 소유권(`own`, 그 안에 `service:board->operate_delete` 가 있다)으로만
// 닿는다. 읽기 부여를 소유권으로 슬쩍 넓혀서 통과시키느니 지운다. 이 공백은 규칙 쪽 결정이라
// 이 단계에서 임의로 메우지 않는다.

