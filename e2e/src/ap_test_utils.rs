//! Grant helpers for the integration suites, written in the vocabulary the evaluator reads.
//!
//! The row tier answers from `relation_tuples`, so every grant here is a tuple. Three grants, one
//! per tier, because the three tiers are three different powers:
//!
//! | tier | helper | means |
//! |---|---|---|
//! | route gate  | [`grant_service`]     | may reach this API at all |
//! | object-wide | [`grant_object_wide`] | may act on **everyone's** rows of this namespace |
//! | one row     | [`grant_object`]      | may act on that one row |
//!
//! Every grant is checked against the compiled rule before it is written ([`assert_rule_reads`]):
//! a tuple on a relation the rule never consults is authority nobody can use, so a fixture that
//! writes one grants **nothing** while every "should be forbidden" assertion keeps passing — the
//! suite reports that a capability still works on the day it stopped existing.

use api::authz::Authz;
use api::authz::subject::SubjectSet;
use api::authz::write::{SubjectKey, TupleWrite};
use reqwest::StatusCode;
use sea_orm::DatabaseConnection;

use crate::prelude::*;

/// Test configuration for permission testing
pub struct PermissionTestConfig {
  pub namespace: &'static str,
  pub base_url:  &'static str,
}

/// Writes one tuple, rule-checked but not authority-checked.
///
/// The authority check belongs to the write *path* (`Authz::write`, `/relation-tuples`); a fixture
/// is the operator, so it seeds directly. The rule check is kept — a relation nobody declared, or a
/// subject type the relation does not accept, is a broken fixture and must say so.
pub async fn tuple(
  db: &DatabaseConnection,
  resource_type: &str,
  resource_id: &str,
  relation: &str,
  subject: SubjectKey,
) -> anyhow::Result<()> {
  Authz::write_unchecked(TupleWrite::new(resource_type, resource_id, relation, subject), "e2e", db).await?;
  Ok(())
}

/// Route-gate tier — the right to reach the API at all. One tuple per action.
///
/// `"*"` expands to the four route actions and no further: on a `service` object the wildcard is
/// not object-wide power, because the object *is* `service`.
pub async fn grant_service(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  actions: Vec<String>,
) -> anyhow::Result<()> {
  for action in expand_actions(&actions) {
    assert_rule_reads("service", &format!("can_{action}"), &action);
    tuple(db, "service", namespace, &action, SubjectKey::user(user.model.id)).await?;
  }
  Ok(())
}

/// Object-wide tier — every row of the namespace, ownership disregarded.
///
/// `service:<namespace>#op_<action>`, which the definition's own permissions union in as
/// `service:<namespace>->operate_<action>`. `"*"` becomes `operator`, the single relation that
/// carries all four.
///
/// **Not every definition has this tier.** `board`, `board_group` and everything that resolves
/// through them (`board_topic`, `board_post`, `board_file`, `board_post_reaction`) union
/// `service:<ns>->operate_delete` into `own` and nothing into `view` or `edit` — so "read every
/// board" is not expressible as a service-tier grant at all, only as ownership. Rather than
/// silently widening a read grant into ownership, this panics and names the gap; those suites grant
/// per board with [`grant_object`].
pub async fn grant_object_wide(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  actions: Vec<String>,
) -> anyhow::Result<()> {
  let definition = definition_of(namespace);

  for action in expand_actions(&actions) {
    // 그 정의에 대응하는 권한 자체가 없으면(예: `sumida_user` 에는 `create` 가 없다) 행 층에
    // 검사가 없다는 뜻이다. 부여할 것이 없으므로 건너뛴다 — 규칙이 읽지 않는 관계에 튜플을
    // 쓰는 것과는 다른 경우다.
    let Some(permission) = permission_of(definition, &action) else { continue };
    let relation = if actions.iter().any(|a| a == "*") {
      "operator".to_owned()
    } else {
      format!("op_{action}")
    };
    assert_rule_reads(definition, permission, &relation);
    tuple(db, "service", namespace, &relation, SubjectKey::user(user.model.id)).await?;
  }
  Ok(())
}

/// One row — authority over a single object.
///
/// The relation is the definition's own, because the tiers are not uniformly named: a board has
/// `viewer`/`editor`/`owner`, an organization has `manager`, a role has `admin`. A namespace with
/// no stored relation at that tier panics rather than writing a tuple nobody reads.
pub async fn grant_object(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  object_id: &str,
  actions: Vec<String>,
) -> anyhow::Result<()> {
  let definition = definition_of(namespace);

  for action in expand_actions(&actions) {
    let Some(permission) = permission_of(definition, &action) else { continue };
    let Some(relation) = object_relation(namespace, &action) else {
      panic!(
        "`{namespace}` declares no stored relation for `{action}` on one row — the rules reach \
         that permission some other way, so this fixture would write a tuple nobody consults"
      );
    };
    assert_rule_reads(definition, permission, relation);
    tuple(db, definition, object_id, relation, SubjectKey::user(user.model.id)).await?;
  }
  Ok(())
}

/// A block on one row, where the rules can express it.
///
/// Denial is a property of the rule and not of a grant: `blocked` is subtracted **outside** the
/// whole union, so authorship cannot walk through it. Only the definitions that declare it
/// (`board_group`, `board`, `board_post`) can carry a block; anywhere else it is not expressible
/// and this panics rather than pretending.
pub async fn block_object(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  object_id: &str,
) -> anyhow::Result<()> {
  let definition = definition_of(namespace);
  assert_rule_reads(definition, "edit", "blocked");
  tuple(db, definition, object_id, "blocked", SubjectKey::user(user.model.id)).await
}

/// An organization-tier grant: `organization:<id>#<family>_<tier>`, reaching every descendant.
///
/// One tuple says "manages this organization and everything under it": the `^` closure reads
/// `org_closure`, so no per-row walk of the tree is needed.
pub async fn grant_org_scope(
  db: &DatabaseConnection,
  user: &TestUser,
  org_id: i32,
  relation: &str,
) -> anyhow::Result<()> {
  tuple(db, "organization", &org_id.to_string(), relation, SubjectKey::user(user.model.id)).await
}

/// Grant both the route gate and the row tier.
///
/// `None` is the object-wide tier: every row of the namespace. `Some(id)` is that one row.
pub async fn grant_service_and_resource_access(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  actions: Vec<String>,
  target_resource_id: Option<i32>,
) -> anyhow::Result<()> {
  grant_service(db, user, namespace, actions.clone()).await?;

  match target_resource_id {
    Some(id) => grant_object(db, user, namespace, &id.to_string(), actions).await,
    None => grant_object_wide(db, user, namespace, actions).await,
  }
}

/// Every route action, plus object-wide power on one row.
pub async fn grant_wildcard_action_permission(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  target_resource_id: Option<i32>,
) -> anyhow::Result<()> {
  grant_service_and_resource_access(db, user, namespace, vec!["*".to_string()], target_resource_id).await
}

/// Object-wide access on every row of a namespace.
pub async fn grant_wildcard_object_permission(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  actions: Vec<String>,
) -> anyhow::Result<()> {
  grant_service(db, user, namespace, actions.clone()).await?;
  grant_object_wide(db, user, namespace, actions).await
}

// ─── the mapping tables ──────────────────────────────────────────────────────────────────────

/// The action vocabulary these fixtures take. `"*"` on a route gate stands for the four route
/// actions.
fn expand_actions(actions: &[String]) -> Vec<String> {
  let mut out: Vec<String> = Vec::new();
  for action in actions {
    if action == "*" {
      out.extend(["create", "read", "update", "delete"].map(str::to_owned));
    } else {
      out.push(action.clone());
    }
  }
  out.sort();
  out.dedup();
  out
}

/// A fixture action → the permission a call site asks for, when the definition declares one.
///
/// `None` is a real answer and not a failure: `sumida_user` has no `create` (a create has no row to
/// check), `board` has no `remove` (deleting a board is `own`). A fixture must not write a tuple for
/// a permission that does not exist, and must not pretend it did.
fn permission_of(definition: &str, action: &str) -> Option<&'static str> {
  let candidates: &[&'static str] = match action {
    "create" => &["create"],
    "update" => &["edit"],
    "delete" => &["remove", "own"],
    _ => &["view"],
  };

  let registry = Authz::registry().expect("the rule set must validate");
  let declared = registry.require(definition).expect("the definition must exist");
  candidates.iter().find(|name| declared.permission(name).is_some()).copied()
}

/// What a namespace's row-level checks resolve to — the `= <definition>` half of the inventory.
///
/// Read out of the registry rather than written down here: a table in the fixture is a second copy
/// of the rule set's inventory, and the two would drift the first time a namespace moved.
/// `Registry::global` is memoised for the process, so the name it hands back lives as long as the
/// process does.
fn definition_of(namespace: &str) -> &'static str {
  let registry = Authz::registry().expect("the rule set must validate");
  let entry = registry
    .namespace(namespace)
    .unwrap_or_else(|| panic!("`{namespace}` is not in the namespace inventory"));
  let definition = entry
    .definition
    .as_deref()
    .unwrap_or_else(|| panic!("`{namespace}` has no row-level definition to grant on"));

  registry
    .require(definition)
    .map(|declared| declared.name.as_str())
    .unwrap_or_else(|e| panic!("`{namespace}` resolves to `{definition}`, which is not declared: {e}"))
}

/// The stored relation that grants one action on one row, per namespace.
///
/// `None` means the definition reaches that permission only through something that is not a
/// grantable relation — a derived column, a walk into another definition, or the service tier.
fn object_relation(namespace: &str, action: &str) -> Option<&'static str> {
  match (namespace, action) {
    ("board" | "board_group", "read") => Some("viewer"),
    ("board" | "board_group", "create" | "update") => Some("editor"),
    ("board" | "board_group", "delete") => Some("owner"),
    // `file#remove` is `owner | org->manage_file | service:file->operate_delete`; the only
    // grantable per-file relation is `viewer`, which reaches `view` alone.
    ("file", "read") => Some("viewer"),
    ("organization", _) => Some("manager"),
    ("role", _) => Some("admin"),
    ("user", _) => Some("manager"),
    _ => None,
  }
}

/// Refuses a grant the rule would never consult.
///
/// This is the most important check in this file. A tuple written on a relation that
/// `<definition>#<permission>` does not read grants nothing at all — the request is refused, the
/// assertion that expected a refusal still passes, and the suite goes green on the day the
/// capability disappeared. That is the shape of the false passes this project has already produced.
///
/// The compiled predicate is the whole decision as text, so "does the rule read this relation" is
/// answerable exactly: the relation's name appears as a literal in it, or it does not.
fn assert_rule_reads(definition: &str, permission: &str, relation: &str) {
  let probe = SubjectSet::from_parts(vec!["user:0".to_owned()], vec![0]);
  let sql = Authz::predicate_sql(definition, permission, &probe)
    .or_else(|_| Authz::object_predicate_sql(definition, "probe", permission, &probe))
    .unwrap_or_else(|error| panic!("`{definition}#{permission}` does not compile: {error}"));

  assert!(
    sql.contains(&format!("'{relation}'")),
    "`{definition}#{permission}` never reads `{relation}`, so a tuple on it would grant nothing \
     and every refusal this fixture expects would pass for the wrong reason:\n  {sql}"
  );
}

/// Test structure for CRUD operations
pub struct CrudTestData {
  pub create_data: serde_json::Value,
  pub update_data: serde_json::Value,
}

/// Comprehensive permission test suite for any controller
pub struct PermissionTestSuite {
  pub config:    PermissionTestConfig,
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
    grant_service_and_resource_access(
      &db,
      &user,
      self.config.namespace,
      vec!["read".to_string()],
      Some(resource_id),
    )
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
  ///
  /// A create has no row to check, so the route gate is the only tier there is.
  pub async fn test_create_permission(&self, mock: &Mock) {
    // Test with permission
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    grant_service(&db, &user, self.config.namespace, vec!["create".to_string()])
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
      Some(resource_id),
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
      Some(resource_id),
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

    grant_service(&db, &user, self.config.namespace, vec!["read".to_string()])
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
  ///
  /// The bypass is `platform:root#superuser`, whose `flagged` branch reads `users.is_admin` — so
  /// nothing is seeded here on purpose. It is the same flag the admin user screen shows, reached
  /// through the rule instead of through a direct session read.
  pub async fn test_admin_bypass(&self, mock: &Mock, resource_id: i32) {
    let db = infra::db::connect().await;
    let admin_user = TestUser::create_admin(&db).await.expect("Failed to create admin user");

    tracing::info!(
      "🔧 Created admin user with ID: {}, is_admin: {}",
      admin_user.model.id,
      admin_user.model.is_admin
    );

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

  /// A block overrides a grant on the same row.
  ///
  /// A block is a `blocked` relation, and only the definitions that declare one can carry it — so
  /// this case exists for the board family, and [`block_object`] refuses anywhere else rather than
  /// quietly asserting nothing.
  pub async fn test_block_precedence(&self, mock: &Mock, resource_id: i32) {
    let db = infra::db::connect().await;
    let user = TestUser::create(&db).await.expect("Failed to create test user");

    grant_service_and_resource_access(
      &db,
      &user,
      self.config.namespace,
      vec!["update".to_string()],
      Some(resource_id),
    )
    .await
    .expect("Failed to grant update permission");

    user.login(&mock.client).await;

    // 차단 전에 실제로 통과하는지부터 본다. 이 확인이 없으면 아래 단언이
    // "원래 못 했다" 로도 통과해 버린다.
    let resp = mock
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(resp.status(), StatusCode::OK, "the grant must work before the block is written");

    block_object(&db, &user, self.config.namespace, &resource_id.to_string())
      .await
      .expect("Failed to write the block");

    let resp = mock
      .client
      .patch(&format!("{}/{}", self.config.base_url, resource_id))
      .json(&self.crud_data.update_data)
      .send()
      .await
      .expect("Failed to send PATCH request");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "a block must override a grant on the same row");
  }

  /// Test wildcard action permissions (`action: ['*']` allows all actions)
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

  /// Object-wide access reaches **every** row of the namespace.
  ///
  /// The acceptance criterion: the grant is `service:<ns>#op_read`, it must open rows the caller
  /// has no per-row grant on, and it must not carry the update tier with it.
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
