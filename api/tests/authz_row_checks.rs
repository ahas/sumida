//! Writes whose route gate is the ordinary tier, held to the row they reach.
//!
//! A route gate says a caller may use an API at all; it says nothing about which accounts, roles,
//! organizations or grants the call may touch. Each case here holds only the gate a tenant's
//! operator would be handed, aims one write at something outside that operator's reach, and asks
//! that the row check — not the gate — refuse it. Each also has the admitted half, inside the
//! operator's reach, so a change that refused the route outright would not pass.
//!
//! A refusal is read as its status and the code naming the rule that refused: the gate's own
//! refusal is `E1009`, and a row refusal is `E1008`.

use api::authz::Authz;
use api::authz::write::SubjectKey;
use e2e::ap_test_utils::{grant_org_scope, grant_service, tuple};
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::Method;
use sea_orm::{ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, Statement};
use serde_json::json;

async fn prepare() -> (Mock, DatabaseConnection) {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  Authz::sync_schema(&db).await.expect("the projection must match the compiled registry");
  (mock, db)
}

fn rnd() -> String {
  api::util::random::generate_url_safe_string(8).unwrap()
}

async fn gates(db: &DatabaseConnection, user: &TestUser, namespace: &str, actions: &[&str]) {
  grant_service(db, user, namespace, actions.iter().map(|a| (*a).to_owned()).collect())
    .await
    .expect("the route gates");
}

/// How many tuples name `subject_key` as their holder.
async fn tuples_held_by(db: &DatabaseConnection, subject_key: &str) -> i64 {
  let row = db
    .query_one(Statement::from_sql_and_values(
      db.get_database_backend(),
      "SELECT COUNT(*) AS n FROM relation_tuples WHERE subject_key = $1",
      [subject_key.into()],
    ))
    .await
    .expect("count the tuples")
    .expect("a count");
  row.try_get::<i64>("", "n").expect("the count")
}

// ─── creating an account ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_a_user_creator_cannot_hand_out_a_role_it_may_not_grant() {
  let (mock, db) = prepare().await;
  let creator = TestUser::create(&db).await.expect("the creator");
  gates(&db, &creator, "user", &["create"]).await;

  // A role that carries platform authority. Roles are global, so its id is anybody's to name.
  let strong = TestRole::create(&db).await.expect("a strong role");
  tuple(
    &db,
    "platform",
    "root",
    "admin",
    SubjectKey::userset("role", &strong.model.id.to_string(), "member"),
  )
  .await
  .expect("the strong role's grant");

  let session = Session::login(mock.server.addr, &creator, "user creator").await;
  let body = |role_id: i32| {
    json!({
      "username": format!("minted-{}", rnd()),
      "password": "known-password",
      "organizationId": creator.model.organization_id,
      "roleId": role_id,
    })
  };

  session
    .probe_write(Method::POST, "/users", Some(&body(strong.model.id)))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  session
    .probe_write(Method::POST, "/users", Some(&body(creator.model.role_id)))
    .await
    .assert(Access::Visible);
}

#[tokio::test]
async fn test_a_new_account_cannot_take_a_removed_accounts_id_or_grants() {
  let (mock, db) = prepare().await;
  let admin = TestUser::create_admin(&db).await.expect("an administrator");
  let departed = TestUser::create(&db).await.expect("the account to remove");
  let departed_id = departed.model.id;
  tuple(&db, "service", "access_policy", "operator", SubjectKey::user(departed_id))
    .await
    .expect("the departed account's grant");

  Session::login(mock.server.addr, &admin, "administrator")
    .await
    .probe_write(Method::DELETE, &format!("/users/{departed_id}"), None)
    .await
    .assert(Access::Visible);

  assert_eq!(
    tuples_held_by(&db, &format!("user:{departed_id}")).await,
    0,
    "the removed account's grants go with it; nothing in the tuple table points at the account to \
     take them along"
  );

  let creator = TestUser::create(&db).await.expect("the creator");
  gates(&db, &creator, "user", &["create"]).await;
  Session::login(mock.server.addr, &creator, "user creator")
    .await
    .probe_write(
      Method::POST,
      "/users",
      Some(&json!({
        "id": departed_id,
        "username": format!("reborn-{}", rnd()),
        "password": "known-password",
        "organizationId": creator.model.organization_id,
        "roleId": creator.model.role_id,
      })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
}

// ─── invitations ───────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_an_invitation_reaches_only_accounts_the_inviter_may_edit() {
  let (mock, db) = prepare().await;
  let operator = TestUser::create(&db).await.expect("the operator");
  gates(&db, &operator, "user", &["update"]).await;
  let outsider = TestUser::create(&db).await.expect("an account elsewhere");

  Session::login(mock.server.addr, &operator, "user operator")
    .await
    .probe_write(Method::POST, &format!("/users/{}/invite", outsider.model.id), None)
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  assert!(
    UserInvitations::find()
      .filter(entity::user_invitations::Column::UserId.eq(outsider.model.id))
      .one(&db)
      .await
      .unwrap()
      .is_none(),
    "no invitation was written for the account out of reach"
  );
}

#[tokio::test]
async fn test_a_disabled_account_is_not_re_enabled_through_an_invitation() {
  let (mock, db) = prepare().await;
  let org = TestOrg::create(&db).await.expect("an organization");
  let manager = TestUser::create_in_org(&db, org.model.id).await.expect("the manager");
  grant_org_scope(&db, &manager, org.model.id, "manager")
    .await
    .expect("the manager's grant");
  gates(&db, &manager, "user", &["update"]).await;

  let disabled = TestUser::create_in_org(&db, org.model.id).await.expect("an account");
  Users::update(entity::users::ActiveModel {
    id: sea_orm::ActiveValue::Unchanged(disabled.model.id),
    is_enabled: sea_orm::ActiveValue::Set(false),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("an administrator disabled it");

  Session::login(mock.server.addr, &manager, "organization manager")
    .await
    .probe_write(Method::POST, &format!("/users/{}/invite", disabled.model.id), None)
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
}

// ─── bans ──────────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_a_ban_that_names_nobody_is_refused() {
  let (mock, db) = prepare().await;
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  Session::login(mock.server.addr, &admin, "administrator")
    .await
    .probe_write(Method::POST, "/bans", Some(&json!({})))
    .await
    .assert(Access::Other(400));
}

#[tokio::test]
async fn test_a_ban_reaches_only_accounts_the_author_may_edit() {
  let (mock, db) = prepare().await;
  let org = TestOrg::create(&db).await.expect("an organization");
  let manager = TestUser::create_in_org(&db, org.model.id).await.expect("the manager");
  grant_org_scope(&db, &manager, org.model.id, "manager")
    .await
    .expect("the manager's grant");
  gates(&db, &manager, "ban", &["create", "update"]).await;
  gates(&db, &manager, "user", &["update"]).await;

  let member = TestUser::create_in_org(&db, org.model.id).await.expect("a member");
  let admin = TestUser::create_admin(&db).await.expect("an administrator elsewhere");
  let session = Session::login(mock.server.addr, &manager, "organization manager").await;

  for path in ["/bans".to_owned(), format!("/users/{}/ban", admin.model.id)] {
    session
      .probe_write(Method::POST, &path, Some(&json!({ "userId": admin.model.id, "reason": "x" })))
      .await
      .assert(Access::Forbidden)
      .assert_code("E1008");
  }
  session
    .probe_write(Method::POST, "/bans", Some(&json!({ "ipAddress": "203.0.113.7" })))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");

  let created = session
    .probe_write(Method::POST, "/bans", Some(&json!({ "userId": member.model.id, "reason": "x" })))
    .await
    .assert(Access::Visible);
  let ban_id = created.body()["id"].as_i64().expect("the ban's id") as i32;

  session
    .probe_write(
      Method::PATCH,
      &format!("/bans/{ban_id}"),
      Some(&json!({ "userId": admin.model.id, "reason": "x" })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");

  let ban = Bans::find_by_id(ban_id).one(&db).await.unwrap().expect("the ban");
  assert_eq!(ban.user_id, Some(member.model.id), "the ban still names the member it was written for");
  assert_eq!(ban.created_by, Some(manager.model.id), "and it records who wrote it");
}

// ─── organizations ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_an_organization_is_moved_only_by_whoever_may_edit_both_parents() {
  let (mock, db) = prepare().await;
  let tree = OrgTree::create(&db).await.expect("the tree");
  let manager = TestUser::create_in_org(&db, tree.subject).await.expect("the manager of subject");
  grant_org_scope(&db, &manager, tree.subject, "manager")
    .await
    .expect("the manager's grant");
  gates(&db, &manager, "organization", &["create", "update"]).await;
  let session = Session::login(mock.server.addr, &manager, "manager of subject").await;

  for parent in [serde_json::Value::Null, json!(tree.aunt)] {
    session
      .probe_write(
        Method::PATCH,
        &format!("/organizations/{}", tree.subject),
        Some(&json!({ "parentId": parent })),
      )
      .await
      .assert(Access::Forbidden)
      .assert_code("E1008");
  }
  let subject = Organizations::find_by_id(tree.subject).one(&db).await.unwrap().unwrap();
  assert_eq!(subject.parent_id, Some(tree.parent), "subject still hangs under its parent");

  session
    .probe_write(Method::POST, "/organizations", Some(&json!({ "name": rnd(), "parentId": tree.aunt })))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  session
    .probe_write(Method::POST, "/organizations", Some(&json!({ "id": 2_000_000_000, "name": rnd() })))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");

  session
    .probe_write(Method::POST, "/organizations", Some(&json!({ "name": rnd(), "parentId": tree.child })))
    .await
    .assert(Access::Visible);
  session
    .probe_write(
      Method::PATCH,
      &format!("/organizations/{}", tree.grandchild),
      Some(&json!({ "parentId": tree.subject })),
    )
    .await
    .assert(Access::Visible);
}

// ─── service grants ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_a_role_editor_cannot_take_away_a_grant_it_could_not_give() {
  let (mock, db) = prepare().await;
  let role = TestRole::create(&db).await.expect("a role");
  let role_member = SubjectKey::userset("role", &role.model.id.to_string(), "member");
  tuple(&db, "service", "user", "operator", role_member.clone())
    .await
    .expect("the role's grant");

  let editor = TestUser::create(&db).await.expect("the role's editor");
  tuple(&db, "role", &role.model.id.to_string(), "admin", SubjectKey::user(editor.model.id))
    .await
    .expect("the editor is the role's admin");
  gates(&db, &editor, "role", &["update"]).await;
  let session = Session::login(mock.server.addr, &editor, "role editor").await;

  session
    .probe_write(
      Method::PATCH,
      &format!("/roles/{}", role.model.id),
      Some(&json!({ "name": role.model.name, "accessPolicies": [] })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  assert_eq!(
    tuples_held_by(&db, &role_member.key()).await,
    1,
    "the grant the editor could never have given is still there"
  );

  session
    .probe_write(
      Method::PATCH,
      &format!("/roles/{}", role.model.id),
      Some(&json!({
        "name": role.model.name,
        "accessPolicies": [
          { "namespace": "user", "object": ["service"], "action": ["operator"], "effect": "allow" }
        ]
      })),
    )
    .await
    .assert(Access::Visible);
}
