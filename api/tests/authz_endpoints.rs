//! The three endpoints of design §14 step 12, over HTTP: `/relation-tuples`, `GET /authz/schema`
//! and `/me/permissions`.
//!
//! Every case here goes through the router, because the thing being asserted is not "does the rule
//! evaluate correctly" — `authz_regressions.rs` and the golden tests already own that — but **what
//! a caller gets back**, which is decided by three layers stacked in a fixed order:
//!
//! 1. `user_guard!` — is there a session at all. `401`.
//! 2. `service_ap!` — does the caller hold `service:access_policy#can_<action>`. `403`, code
//!    `E1009`. It runs **before the handler**, so nothing about the request body has been looked at.
//! 3. `Authz::write` / `Authz::revoke` / the list's own predicate — does the caller hold
//!    `<resource_type>#grant` **on the object named in the request**. `403`, code `E1008`.
//!
//! Layer 2 is the trap this file is written around. It refuses first and it refuses with a `403`,
//! so a case that forgets to grant its caller the service tier is green whether or not layer 3
//! exists at all — the whole row scope could be deleted and the suite would not notice. Every
//! negative case below therefore **grants the service tier first** and then asserts on the code,
//! never on the status alone.
//!
//! Requires a real Postgres, and rewrites the vocabulary projection once per binary so the tuple
//! table's foreign key will accept a grant on any relation the rules declare.

use std::collections::BTreeSet;

use api::authz::Authz;
use e2e::prelude::*;
use reqwest::Method;
use sea_orm::{ConnectionTrait, DatabaseConnection, FromQueryResult, Statement};
use serde_json::{Value, json};
use tokio::sync::OnceCell;

// ─── harness ─────────────────────────────────────────────────────────────────────────────────

static VOCABULARY: OnceCell<()> = OnceCell::const_new();

/// A server, a connection, and a vocabulary projection that matches the compiled registry.
///
/// The projection has to be rewritten because the migration seeds only the subset the backfill
/// writes (`rebac.rs`), and `fk_relation_tuples__relation` refuses everything else. Without it a
/// case that grants `organization#manager` fails on a foreign key and reads as an authorization
/// refusal, which is the least useful false positive available.
async fn prepare() -> (Mock, DatabaseConnection) {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  VOCABULARY
    .get_or_init(|| async {
      Authz::sync_schema(&db).await.expect("the projection must match the compiled registry");
    })
    .await;

  (mock, db)
}

/// Writes one tuple straight into the table and returns its id.
///
/// Deliberately raw SQL and not `Authz::write`: a fixture that grants through the code under test
/// cannot fail a mutation of that code — it would fail to *set up*, which is a different colour of
/// red and one that hides which assertion actually broke.
async fn grant(
  db: &DatabaseConnection,
  resource_type: &str,
  resource_id: &str,
  relation: &str,
  subject: &str,
) -> i64 {
  #[derive(FromQueryResult)]
  struct Row {
    id: i64,
  }

  let (head, subject_relation) = subject.split_once('#').unwrap_or((subject, ""));
  let (subject_type, subject_id) = head.split_once(':').expect("a subject key is `<type>:<id>`");

  Row::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "INSERT INTO relation_tuples
       (resource_type, resource_id, relation, subject_type, subject_id, subject_relation, created_by)
     VALUES ($1, $2, $3, $4, $5, $6, 'fixture')
     ON CONFLICT (resource_type, resource_id, relation, subject_type, subject_id, subject_relation)
     DO UPDATE SET created_by = 'fixture'
     RETURNING id",
    [
      resource_type.into(),
      resource_id.into(),
      relation.into(),
      subject_type.into(),
      subject_id.into(),
      subject_relation.into(),
    ],
  ))
  .one(db)
  .await
  .expect("write a fixture tuple")
  .expect("the insert returns its id")
  .id
}

/// `GET` on an endpoint that answers with a document rather than a resource.
///
/// `probe_one` classifies a `2xx` by whether an id appears in the body, which is the right question
/// for a record and the wrong one for `/me/permissions` — it has no id and would read as
/// [`Access::Filtered`] forever. `probe_write` classifies on the status alone, which is what these
/// two want.
async fn get_document(session: &Session, path: &str) -> Probe {
  session.probe_write(Method::GET, path, None).await
}

fn tuple_body(resource_type: &str, resource_id: &str, relation: &str, subject: &str) -> Value {
  json!({
    "resourceType": resource_type,
    "resourceId": resource_id,
    "relation": relation,
    "subject": subject,
  })
}

fn verbs(body: &Value, map: &str, namespace: &str) -> Vec<String> {
  body
    .get(map)
    .and_then(|m| m.get(namespace))
    .and_then(Value::as_array)
    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
    .unwrap_or_default()
}

fn namespaces(body: &Value, map: &str) -> BTreeSet<String> {
  body
    .get(map)
    .and_then(Value::as_object)
    .map(|m| m.keys().cloned().collect())
    .unwrap_or_default()
}

const ALL: [&str; 4] = ["create", "read", "update", "delete"];

// ─── /me/permissions ─────────────────────────────────────────────────────────────────────────

/// A platform administrator holds every gated namespace, at both tiers.
///
/// The assertion is against the **inventory**, not a count and not a sample. "38 namespaces" passes
/// just as happily when one of them has been swapped out, and the claim R1 makes is *which* ones:
/// every namespace that has a route gate at all, because `operate_X` unions in
/// `platform:root->superuser` and `can_X` unions in `operate_X`.
///
/// Nobody wrote a `service:` tuple for this user. That is the point — the entry comes from the
/// rule, so a namespace nobody has ever granted anything on is still reachable by an administrator.
#[tokio::test]
async fn a_platform_administrator_holds_every_gated_namespace() {
  let (mock, db) = prepare().await;

  let admin = TestUser::create_admin(&db).await.expect("an administrator");
  let session = Session::login(mock.server.addr, &admin, "platform admin").await;
  session.assert_authenticated().await;

  let probe = get_document(&session, "/me/permissions").await.assert(Access::Visible);
  let body = probe.body();

  assert_eq!(body.get("superuser"), Some(&Value::Bool(true)), "body: {body}");

  let gated: BTreeSet<String> = Authz::registry()
    .expect("the rule set must validate")
    .namespaces()
    .iter()
    .filter(|entry| entry.gated)
    .map(|entry| entry.namespace.clone())
    .collect();

  assert_eq!(namespaces(body, "services"), gated, "the route-gate tier is not the whole inventory");
  assert_eq!(namespaces(body, "operates"), gated, "the object-wide tier is not the whole inventory");

  for namespace in &gated {
    assert_eq!(verbs(body, "services", namespace), ALL, "services[{namespace}]");
    assert_eq!(verbs(body, "operates", namespace), ALL, "operates[{namespace}]");
  }
}

/// A plain member holds nothing, and is told so as an empty map rather than an error.
///
/// The `subjects` list is asserted exactly, because it is the one field a client cannot check for
/// itself and the one an API-key session legitimately changes: a secured key is judged as
/// `apikey:<hash>` **alone**, with no user, role or organization behind it.
#[tokio::test]
async fn a_plain_member_holds_nothing() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let member = Member::create(&db, tree.subject).await.expect("a plain member");
  let session = Session::login(mock.server.addr, &member.user, "plain member").await;
  session.assert_authenticated().await;

  let probe = get_document(&session, "/me/permissions").await.assert(Access::Visible);
  let body = probe.body();

  assert_eq!(body.get("superuser"), Some(&Value::Bool(false)), "body: {body}");
  assert!(namespaces(body, "services").is_empty(), "body: {body}");
  assert!(namespaces(body, "operates").is_empty(), "body: {body}");

  assert_eq!(
    body.get("subjects"),
    Some(&json!([
      format!("user:{}", member.user.model.id),
      format!("role:{}#member", member.user.model.role_id),
      format!("organization:{}#member", member.user.model.organization_id),
    ])),
    "the identity a request is judged as is wrong, so every other answer here is about somebody else"
  );
}

/// An organization manager sees the route gates they were granted — and **not** their organization
/// authority, which is not a service-tier capability at all.
///
/// This is the case that keeps the two maps apart. The manager tuple is real authority: it is what
/// lets this person grant inside their subtree two cases below. It is simply not a `service:` grant,
/// so it must not appear in `services`, and it must not appear in `operates` either — `operates` is
/// "may act on everyone's rows of this namespace", which is a different thing from "manages an
/// organization".
#[tokio::test]
async fn an_organization_manager_sees_the_route_gates_and_not_the_org_authority() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let manager = Member::create(&db, tree.subject).await.expect("the manager");
  let key = format!("user:{}", manager.user.model.id);

  grant(&db, "organization", &tree.subject.to_string(), "manager", &key).await;
  grant(&db, "service", "access_policy", "create", &key).await;

  let session = Session::login(mock.server.addr, &manager.user, "org-subject manager").await;
  let probe = get_document(&session, "/me/permissions").await.assert(Access::Visible);
  let body = probe.body();

  assert_eq!(body.get("superuser"), Some(&Value::Bool(false)), "body: {body}");
  assert_eq!(
    namespaces(body, "services"),
    BTreeSet::from(["access_policy".to_owned()]),
    "body: {body}"
  );
  assert_eq!(verbs(body, "services", "access_policy"), ["create"], "body: {body}");
  assert!(
    namespaces(body, "operates").is_empty(),
    "a route-gate grant leaked into the object-wide tier, which is the grant that lets somebody act \
     on everyone's rows. body: {body}"
  );
}

/// One `operator` tuple reports all eight capabilities, because the **rule** says so.
///
/// The payload is evaluated through `service#can_X` / `service#operate_X`, not read off the tuples.
/// A projection that reported the raw relations would answer `{"user": ["operator"]}` here, and the
/// client would have to re-derive `can_read = read | op_read | operator | superuser` in TypeScript —
/// a second copy of `schema.zed`, drifting.
#[tokio::test]
async fn one_operator_tuple_reports_every_verb() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let operator = Member::create(&db, tree.subject).await.expect("an operator");
  let key = format!("user:{}", operator.user.model.id);

  grant(&db, "service", "user", "operator", &key).await;

  let session = Session::login(mock.server.addr, &operator.user, "user-namespace operator").await;
  let probe = get_document(&session, "/me/permissions").await.assert(Access::Visible);
  let body = probe.body();

  assert_eq!(verbs(body, "operates", "user"), ALL, "body: {body}");
  assert_eq!(
    verbs(body, "services", "user"),
    ALL,
    "`can_X = X | operate_X`, so an operator passes every route gate on the namespace. body: {body}"
  );
}

// ─── /relation-tuples: who may write ─────────────────────────────────────────────────────────

/// The two `403`s are different refusals, and a case that cannot tell them apart is asserting
/// nothing.
///
/// Without the service tier the route gate refuses (`E1009`) and the handler never runs, so the
/// authority check on the object was never exercised. With the service tier the request reaches
/// `Authz::write` and is refused there (`E1008`). A suite that accepted "denied somehow" would pass
/// on both while the entire authority check was missing.
#[tokio::test]
async fn the_route_gate_and_the_object_authority_are_different_refusals() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let member = Member::create(&db, tree.subject).await.expect("a plain member");
  let key = format!("user:{}", member.user.model.id);
  let session = Session::login(mock.server.addr, &member.user, "plain member").await;
  let body = tuple_body("service", "user", "read", &key);

  session
    .probe_write(Method::POST, "/relation-tuples", Some(&body))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1009");

  // Now the caller may reach the endpoint. Nothing about their authority over `service:user` has
  // changed, so the refusal must move to the other layer rather than disappear.
  grant(&db, "service", "access_policy", "create", &key).await;

  session
    .probe_write(Method::POST, "/relation-tuples", Some(&body))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");

  DbState::new(&db)
    .assert_absent(
      "relation_tuples",
      &format!(
        "resource_type = 'service' AND resource_id = 'user' AND relation = 'read' AND subject_key = '{key}'"
      ),
    )
    .await;
}

/// A member cannot grant themselves anything they do not already have the authority to grant.
///
/// Both halves matter and they fail for different reasons. `service:trainer:task#op_read` is the
/// "let me see everyone's tasks" grant, refused because `service#grant` is
/// `operator | platform:root->superuser` and this caller is neither. `platform:root#admin` is
/// outright self-elevation to platform administrator, refused because `platform#grant` is
/// `superuser`.
///
/// Both are attempted **with the service tier already granted**, so the refusal is the object
/// authority and not the gate — asserted by code, not by status.
///
/// The old rule here was `assert_policy_within_authority`, which refused a non-admin only when the
/// row contained a `*`. `trainer:task | * | {read} | user:<self>` was refused by it. `platform` had no
/// representation at all, and a concrete row naming somebody else's resource passed it outright.
#[tokio::test]
async fn a_member_cannot_grant_themselves_more_than_they_hold() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let member = Member::create(&db, tree.subject).await.expect("a plain member");
  let key = format!("user:{}", member.user.model.id);

  grant(&db, "service", "access_policy", "create", &key).await;

  let session = Session::login(mock.server.addr, &member.user, "plain member").await;
  let state = DbState::new(&db);

  session
    .probe_write(
      Method::POST,
      "/relation-tuples",
      Some(&tuple_body("service", "trainer:task", "op_read", &key)),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  state
    .assert_absent(
      "relation_tuples",
      &format!("resource_type = 'service' AND resource_id = 'trainer:task' AND subject_key = '{key}'"),
    )
    .await;

  session
    .probe_write(Method::POST, "/relation-tuples", Some(&tuple_body("platform", "root", "admin", &key)))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  state
    .assert_absent(
      "relation_tuples",
      &format!("resource_type = 'platform' AND relation = 'admin' AND subject_key = '{key}'"),
    )
    .await;
}

/// Holding authority somewhere is not holding it everywhere.
///
/// The caller manages `sibling` and has the service tier, so both of the earlier layers are
/// satisfied and the only thing left to refuse is the object. `child` is in the other branch of the
/// tree and `parent` is an ancestor of the caller's own organization — R2 goes down, never up and
/// never sideways, so both must be refused with `E1008`.
#[tokio::test]
async fn a_member_cannot_grant_on_an_object_outside_their_scope() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let manager = Member::create(&db, tree.sibling).await.expect("a manager of the sibling branch");
  let victim = Member::create(&db, tree.child).await.expect("somebody in the other branch");
  let key = format!("user:{}", manager.user.model.id);
  let target = format!("user:{}", victim.user.model.id);

  grant(&db, "organization", &tree.sibling.to_string(), "manager", &key).await;
  grant(&db, "service", "access_policy", "create", &key).await;

  let session = Session::login(mock.server.addr, &manager.user, "org-sibling manager").await;
  let state = DbState::new(&db);

  // `child` is the other branch of the tree; `parent` is an ancestor of the caller's own
  // organization. Neither is reachable from a `manager` tuple on `sibling`.
  for organization in [tree.child, tree.parent] {
    session
      .probe_write(
        Method::POST,
        "/relation-tuples",
        Some(&tuple_body("organization", &organization.to_string(), "manager", &target)),
      )
      .await
      .assert(Access::Forbidden)
      .assert_code("E1008");

    state
      .assert_absent(
        "relation_tuples",
        &format!(
          "resource_type = 'organization' AND resource_id = '{organization}' AND relation = 'manager' \
           AND subject_key = '{target}'"
        ),
      )
      .await;
  }
}

/// An organization manager grants inside their subtree, to arbitrary depth, and nowhere else.
///
/// `grandchild` is two levels below the managed organization, which is what separates a closure
/// from a one-level join — the two agree at depth 1 and diverge below it. `parent` and `sibling` are
/// the controls: an inverted walk hands a manager authority over their own ancestors, and a test
/// that only checks the descendant half passes either way.
#[tokio::test]
async fn an_organization_manager_grants_inside_the_subtree_and_not_outside() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let manager = Member::create(&db, tree.subject).await.expect("the manager");
  let victim = Member::create(&db, tree.child).await.expect("the person being granted to");
  let key = format!("user:{}", manager.user.model.id);
  let target = format!("user:{}", victim.user.model.id);

  grant(&db, "organization", &tree.subject.to_string(), "manager", &key).await;
  grant(&db, "service", "access_policy", "create", &key).await;

  let session = Session::login(mock.server.addr, &manager.user, "org-subject manager").await;
  let state = DbState::new(&db);

  session
    .probe_write(
      Method::POST,
      "/relation-tuples",
      Some(&tuple_body("organization", &tree.grandchild.to_string(), "manager", &target)),
    )
    .await
    .assert(Access::Visible);

  state
    .assert_exists(
      "relation_tuples",
      &format!(
        "resource_type = 'organization' AND resource_id = '{}' AND relation = 'manager' \
         AND subject_key = '{target}' AND created_by = '{key}'",
        tree.grandchild
      ),
    )
    .await;

  for outside in [tree.parent, tree.sibling] {
    session
      .probe_write(
        Method::POST,
        "/relation-tuples",
        Some(&tuple_body("organization", &outside.to_string(), "manager", &target)),
      )
      .await
      .assert(Access::Forbidden)
      .assert_code("E1008");

    state
      .assert_absent(
        "relation_tuples",
        &format!(
          "resource_type = 'organization' AND resource_id = '{outside}' AND relation = 'manager' \
           AND subject_key = '{target}'"
        ),
      )
      .await;
  }
}

/// Revocation removes the authority, not just the row.
///
/// The row going is the easy half and a `DELETE` that ran against the wrong table would fail it.
/// What this asserts instead is the consequence: the revoked manager, who a moment ago could grant
/// inside their subtree, is refused with `E1008` afterwards. A revocation that deleted a *duplicate*
/// and left the original — which `idx_access_policies__tuple`, not being unique, allowed — passes
/// the row assertion and fails this one.
#[tokio::test]
async fn revocation_actually_revokes() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let owner = Member::create(&db, tree.subject).await.expect("the manager of the subtree");
  let deputy = Member::create(&db, tree.child).await.expect("the deputy being promoted and demoted");
  let target = Member::create(&db, tree.grandchild).await.expect("somebody to grant to");

  let owner_key = format!("user:{}", owner.user.model.id);
  let deputy_key = format!("user:{}", deputy.user.model.id);
  let target_key = format!("user:{}", target.user.model.id);

  grant(&db, "organization", &tree.subject.to_string(), "manager", &owner_key).await;
  grant(&db, "service", "access_policy", "create", &owner_key).await;
  grant(&db, "service", "access_policy", "delete", &owner_key).await;
  grant(&db, "service", "access_policy", "create", &deputy_key).await;

  let deputy_grant = grant(&db, "organization", &tree.child.to_string(), "manager", &deputy_key).await;

  let owner_session = Session::login(mock.server.addr, &owner.user, "subtree manager").await;
  let deputy_session = Session::login(mock.server.addr, &deputy.user, "deputy").await;
  let state = DbState::new(&db);

  // The deputy really does hold the authority, or the revocation below would be revoking nothing.
  deputy_session
    .probe_write(
      Method::POST,
      "/relation-tuples",
      Some(&tuple_body("organization", &tree.grandchild.to_string(), "user_viewer", &target_key)),
    )
    .await
    .assert(Access::Visible);

  owner_session
    .probe_write(Method::DELETE, &format!("/relation-tuples/{deputy_grant}"), None)
    .await
    .assert(Access::Visible);

  // The consequence first, the record second. A revocation that left the grant standing would fail
  // both, and asserting the row first would let it fail there — leaving the question this case
  // actually asks, "is the authority gone", never reached.
  deputy_session
    .probe_write(
      Method::POST,
      "/relation-tuples",
      Some(&tuple_body("organization", &tree.grandchild.to_string(), "user_editor", &target_key)),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");

  state
    .assert_absent(
      "relation_tuples",
      &format!(
        "resource_type = 'organization' AND resource_id = '{}' AND relation = 'user_editor'",
        tree.grandchild
      ),
    )
    .await;

  state.assert_absent("relation_tuples", &format!("id = {deputy_grant}")).await;
}

// ─── /relation-tuples: what may be read ──────────────────────────────────────────────────────

/// The list is scoped by the rule, and the caller who is scoped out gets an **empty list**, not a
/// refusal.
///
/// Both callers pass the route gate — one holds `service:access_policy#read`, the other holds
/// `#op_read`, which implies it. So the `403`/`200` question is settled before the row scope runs,
/// and the only thing separating the two responses is `relation_tuple#view`. That is the exact
/// shape of leak a status-code assertion cannot see: the reader gets `200 OK` either way.
#[tokio::test]
async fn the_list_is_scoped_by_the_rule_and_not_by_the_gate() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let reader = Member::create(&db, tree.subject).await.expect("a caller with the gate only");
  let operator = Member::create(&db, tree.subject).await.expect("a caller with the object-wide tier");
  let subject_of_grant = Member::create(&db, tree.child).await.expect("somebody a grant names");

  grant(&db, "service", "access_policy", "read", &format!("user:{}", reader.user.model.id)).await;
  grant(&db, "service", "access_policy", "op_read", &format!("user:{}", operator.user.model.id)).await;

  let listed = grant(
    &db,
    "organization",
    &tree.subject.to_string(),
    "manager",
    &format!("user:{}", subject_of_grant.user.model.id),
  )
  .await;

  // Scoped to this test's own organization, because the suite shares one database and a bare list
  // would be a race against every other case's fixtures.
  let path = format!("/relation-tuples?resourceType=organization&resourceId={}", tree.subject);

  Session::login(mock.server.addr, &reader.user, "route gate only")
    .await
    .probe_list(&path)
    .await
    .assert_empty();

  Session::login(mock.server.addr, &operator.user, "object-wide reader")
    .await
    .probe_list(&path)
    .await
    .assert_exactly(&[&listed]);
}

// ─── GET /authz/schema ───────────────────────────────────────────────────────────────────────

/// Any authenticated caller may read the vocabulary, including one who holds no policy capability
/// at all.
///
/// That is the whole justification for not gating it on `service:access_policy#can_read`: the
/// `/relation-tuples` form needs the relation names to render its selects before the server has
/// told it anything, and any screen that displays a permission needs the vocabulary to name it.
/// Gating the dictionary behind the capability to edit its entries leaves those callers hardcoding
/// relation strings, which is the state this endpoint exists to end.
///
/// It also pins what the payload does **not** carry. `kind` says a relation is derived; the
/// derivation — its table, its column, its join hops, its raw SQL filter — stays on the server.
#[tokio::test]
async fn the_schema_is_readable_by_any_authenticated_caller_and_names_no_table() {
  let (mock, db) = prepare().await;

  let tree = OrgTree::create(&db).await.expect("an organization tree");
  let nobody = Member::create(&db, tree.subject).await.expect("a caller with no capability at all");
  let session = Session::login(mock.server.addr, &nobody.user, "plain member").await;
  session.assert_authenticated().await;

  let probe = get_document(&session, "/authz/schema").await.assert(Access::Visible);
  let body = probe.body();

  let relations = body.get("relations").and_then(Value::as_array).expect("relations");
  let find = |resource_type: &str, relation: &str| {
    relations
      .iter()
      .find(|r| {
        r.get("resourceType").and_then(Value::as_str) == Some(resource_type)
          && r.get("relation").and_then(Value::as_str) == Some(relation)
      })
      .cloned()
      .unwrap_or_else(|| panic!("`{resource_type}#{relation}` is missing from the vocabulary"))
  };

  let manager = find("organization", "manager");
  assert_eq!(manager.get("kind").and_then(Value::as_str), Some("stored"));
  assert!(
    manager
      .get("subjectTypes")
      .and_then(Value::as_array)
      .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("user"))),
    "the subject select would have nothing to offer: {manager}"
  );

  let member = find("organization", "member");
  assert_eq!(
    member.get("kind").and_then(Value::as_str),
    Some("derived"),
    "a derived relation offered as grantable is a grant the evaluator never reads"
  );

  assert!(
    body.get("permissions").and_then(Value::as_array).is_some_and(|p| !p.is_empty()),
    "the screen cannot say *why* a permission is held without the rules"
  );

  let raw = body.to_string();
  for leaked in ["users.is_admin", "organizations.parent_id", "\"table\"", "\"column\""] {
    assert!(!raw.contains(leaked), "`GET /authz/schema` leaked `{leaked}`");
  }
}

/// The vocabulary is not enumerable without a session.
///
/// It carries no grant, so there is no authorization state to leak — but it does enumerate the
/// whole resource model, and every other route in this tree requires a session. Requiring one here
/// costs nothing and keeps the surface uniform.
#[tokio::test]
async fn the_schema_is_not_readable_anonymously() {
  let (mock, _db) = prepare().await;

  get_document(&Session::anonymous(mock.server.addr), "/authz/schema")
    .await
    .assert(Access::Unauthenticated);
}
