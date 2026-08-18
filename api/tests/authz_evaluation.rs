//! What the compiled rules answer, against a real database, per mechanism.
//!
//! This file is what is left of `api/tests/authz_equivalence.rs` after the legacy evaluator was
//! deleted (§14 step 14). That file ran `ap.rs` and `Authz` side by side over the same Postgres and
//! asserted the two agreed; it was the acceptance evidence for the migration, and it could not
//! outlive the thing on its left-hand side. So every case that still says something was rewritten
//! as an **absolute** claim — the answer the legacy evaluator used to supply is now written down as
//! the expectation — and the cases whose only content was "the two agree" went with it.
//!
//! Six mechanisms, chosen because each is a different one rather than a different namespace:
//!
//! | area | rule |
//! |---|---|
//! | service-tier route gate | `service:<ns>#can_<a>`, and its narrowing against `operate_<a>` |
//! | delegation | `delegation_scope:<scope>#assert`, matched by equality and never by prefix |
//! | organization hierarchy | `sumida_user:<id>#edit` through the `^` closure |
//! | board allow/deny composition | `board_post:<id>#edit` — an intersection inside, an exclusion outside |
//! | the object tier, per converted call site | `<definition>:<id>#<permission>` |
//! | lab-app's object tier | the sharing levels, the walks, and the organization filter |
//!
//! Each case prints a row of `expected | actual` and then asserts. The table is kept because it is
//! the artefact a reviewer reads: a bare `assert!` says a rule is wrong without saying which of a
//! dozen shapes it was wrong about. Where a case records a **deliberate** difference from the old
//! model — §1.5's widenings and narrowings — the expectation *is* the new answer and the `note`
//! says which one it is and why, because a difference nobody wrote down is a difference nobody
//! decided.
//!
//! Two things are seeded here as `access_policies` rows rather than as tuples, and both on purpose:
//! [`the_mirror_writes_what_the_backfill_would_have`], whose subject is that mapping, and
//! [`daqs_site_organization_filter_asks_the_same_question`], which compares the raw SQL lab-app used
//! to run against the raw SQL it runs now. Everywhere else the fixture writes the store the rules
//! read, because a test of a rule that seeds through a translation layer goes red for the layer's
//! reasons as well as the rule's.
//!
//! Requires a real Postgres. Nothing in the request path is exercised except
//! [`me_access_policies_keeps_its_payload`]: the evaluator is called directly, so a failure here is
//! a failure in the rules and not in a handler.

use api::authz::subject::SubjectSet;
use api::authz::{Authz, DefinitionState};
use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  DatabaseConnection,
  EntityTrait,
  FromQueryResult,
  QueryFilter,
  Statement,
};

// ─── harness ─────────────────────────────────────────────────────────────────────────────────

/// One claim about the rules, and what they actually said.
struct Case {
  what:     String,
  expected: bool,
  actual:   bool,
  /// Set when this case records a deliberate change from the pre-migration model. It does not
  /// weaken the assertion — `expected` is still asserted — it says *why* the expectation is what
  /// it is, where a reader of the old system would have predicted the other answer.
  note:     Option<&'static str>,
}

impl Case {
  fn new(what: impl Into<String>, expected: bool, actual: bool) -> Self {
    Self {
      what: what.into(),
      expected,
      actual,
      note: None,
    }
  }

  fn noting(mut self, note: &'static str) -> Self {
    self.note = Some(note);
    self
  }
}

/// Renders the claims as a table and then asserts every one of them.
///
/// The table is printed whether or not the assertions hold, because "which of these shapes the rule
/// got wrong" is the thing worth reading, and a `assert_eq!` on the first failure hides the rest.
fn report(area: &str, cases: &[Case]) {
  println!("\n### {area}\n");
  println!("| case | expected | actual | verdict |");
  println!("|---|---|---|---|");
  for case in cases {
    let verdict = match (case.expected == case.actual, case.note) {
      (true, None) => "ok".to_owned(),
      (true, Some(note)) => format!("ok — {note}"),
      (false, _) => "**WRONG**".to_owned(),
    };
    println!("| {} | {} | {} | {verdict} |", case.what, case.expected, case.actual);
  }
  println!();

  for case in cases {
    assert_eq!(
      case.expected, case.actual,
      "`{}`: the rule must answer {}, and it answered {}",
      case.what, case.expected, case.actual
    );
  }
}

/// One caller's identity, without needing a session.
///
/// `is_admin` is deliberately **not** an extra principal: R1 is `platform#flagged`, a derived
/// tupleset over `users.is_admin`, so the flag enters through `user_ids`. This is
/// `SubjectSet::of`'s non-api-key branch, spelled out against a `users` row.
fn subject_of(user: &entity::users::Model) -> SubjectSet {
  SubjectSet::from_parts(
    vec![
      format!("user:{}", user.id),
      format!("role:{}#member", user.role_id),
      format!("organization:{}#member", user.organization_id),
    ],
    vec![user.id],
  )
}

/// Writes one legacy policy row.
///
/// Only two tests still use it, and for both the row **is** the input under test: the mirror's
/// mapping, and lab-app's organization filter. Nothing else seeds this way — the rules do not read
/// this table and have not since §14 step 13, so a grant written here reaches an evaluator only by
/// way of the mirror trigger, and a test that depends on that silently depends on the trigger too.
async fn legacy_policy(
  db: &DatabaseConnection,
  user: &TestUser,
  namespace: &str,
  object: &str,
  actions: Vec<String>,
  effect: &str,
) {
  entity::access_policies::Entity::insert(entity::access_policies::ActiveModel {
    namespace: Set(namespace.to_owned()),
    object: Set(object.to_owned()),
    action: Set(actions),
    effect: Set(effect.to_owned()),
    user: Set(format!("user:{}", user.model.id)),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("write a legacy policy row");
}

/// Writes one tuple. `Authz::write` — with its authority check and `created_by` stamp — belongs to
/// the write path's own suite (`authz_endpoints.rs`); here the point is what the *reader* does with
/// a tuple that exists.
async fn tuple(
  db: &DatabaseConnection,
  resource_type: &str,
  resource_id: &str,
  relation: &str,
  subject_type: &str,
  subject_id: &str,
  subject_relation: &str,
) {
  db.execute(Statement::from_sql_and_values(
    db.get_database_backend(),
    "INSERT INTO relation_tuples
       (resource_type, resource_id, relation, subject_type, subject_id, subject_relation, created_by)
     VALUES ($1, $2, $3, $4, $5, $6, 'test') ON CONFLICT DO NOTHING",
    [
      resource_type.into(),
      resource_id.into(),
      relation.into(),
      subject_type.into(),
      subject_id.into(),
      subject_relation.into(),
    ],
  ))
  .await
  .expect("write a tuple");
}

/// `tuple`, for the common case of a `user:<id>` subject.
async fn grant(db: &DatabaseConnection, resource_type: &str, resource_id: &str, relation: &str, user: &TestUser) {
  tuple(db, resource_type, resource_id, relation, "user", &user.model.id.to_string(), "").await;
}

/// Brings the vocabulary projection up to date, so a tuple on any relation the rules declare can be
/// written at all. The bootstrap seed in the `rebac` migration covers only what the backfill needs.
async fn prepare() -> DatabaseConnection {
  // Loads `.env.e2e` and wipes/re-migrates the test database, once per binary — the same entry the
  // other suites use. The server itself is never started except by the `/me` case.
  e2e::bootstrap::init_once().await;
  let db = infra::db::connect().await;
  Authz::sync_schema(&db).await.expect("the projection must match the compiled registry");
  db
}

// ─── the boot contract ───────────────────────────────────────────────────────────────────────

/// Boot validation runs, and says which rules this database can actually back.
///
/// sumida's own definitions must be **active**; trainer's, site-app's and lab-app's must be **absent**,
/// because their tables are not in this database. An `Absent` is not a failure — sumida is one
/// working tree shared by four products and no database has all four — but a `Drift` is: it means a
/// rule reads a column of a table that *is* here and does not have it.
#[tokio::test]
async fn boot_reports_which_rules_this_database_can_back() {
  let db = prepare().await;
  let report = Authz::boot(&db).await.expect("the rule set must validate");

  for definition in ["board", "board_post", "organization", "sumida_user", "file", "service"] {
    assert_eq!(
      report.state_of(definition),
      Some(&DefinitionState::Active),
      "`{definition}` is sumida's own and every column it reads is here"
    );
  }

  for definition in ["joy_task", "antares_project", "daqs_site"] {
    assert!(
      matches!(report.state_of(definition), Some(DefinitionState::Absent { .. })),
      "`{definition}` belongs to a product that is not deployed in this database, and boot said {:?}",
      report.state_of(definition)
    );
  }

  let drifted: Vec<_> = report.drifted().map(|entry| entry.definition.clone()).collect();
  assert!(drifted.is_empty(), "a rule reads a column this database does not have: {drifted:?}");
}

// ─── 1. the service-tier route gate ──────────────────────────────────────────────────────────

/// `service_ap!(state, "user", read)` is `service:user#can_read`, and the two tiers stay apart.
///
/// The deliberate semantic change of §5.2 is asserted rather than left implicit. The old
/// `_build_object_condition` ORed `object = '*'` unconditionally, so a row granting object-wide
/// power also satisfied the route gate; here `operate_read` still implies `can_read`, and a gate
/// grant still does **not** imply object-wide power. Both directions are cases of their own,
/// because a rule that collapsed them would pass a test that only checked the first.
#[tokio::test]
async fn the_route_gate_and_the_object_wide_tier_are_different_grants() {
  let db = prepare().await;

  let nobody = TestUser::create(&db).await.expect("a user with no grants");
  let gated = TestUser::create(&db).await.expect("a user with the route gate");
  let operator = TestUser::create(&db).await.expect("a user with object-wide power");
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  grant(&db, "service", "user", "read", &gated).await;
  grant(&db, "service", "user", "op_read", &operator).await;
  // The administrator gets nothing at all: `platform#flagged` derives from `users.is_admin`, so the
  // grant is the column. The old model needed a `* | * | {*} | user:admin` catch-all row for this,
  // and that row is superseded rather than migrated (§2.2).

  let can_read = |user: &TestUser| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move { Authz::check("service", "user", "can_read", &subject, &db).await.expect("runs") }
  };

  let cases = vec![
    Case::new("no grant reaches the gate", false, can_read(&nobody).await),
    Case::new("a route-gate grant opens the gate", true, can_read(&gated).await),
    Case::new("an object-wide grant also opens the gate", true, can_read(&operator).await),
    Case::new("an administrator holds every gate", true, can_read(&admin).await).noting(
      "from `platform:root#superuser` over `users.is_admin`, not from the catch-all policy row",
    ),
    Case::new(
      "a route-gate grant confers object-wide read",
      false,
      Authz::check("service", "user", "operate_read", &subject_of(&gated.model), &db)
        .await
        .expect("runs"),
    )
    .noting(
      "§5.2's split: \"may use the user API\" stopped being the same grant as \"may see everyone\"",
    ),
  ];

  report("the service-tier route gate", &cases);
}

/// A route gate is an **object id** on `service`, and it opens exactly the one namespace it names.
///
/// This case used to assert that the Phase 1 shim really consulted the compiled rule for a routed
/// namespace, by granting a tuple no `access_policies` row backed. There is no shim and no second
/// store to fall back to, so what it asserts now is the half that was never about the shim: one
/// namespace per grant, and no leakage between the four products that share this rule set.
///
/// **The negative is not the one it looks like it should be, and that is worth stating.** The
/// compiled evaluator has no notion of a *gated* namespace: `service` has no backing table, so its
/// object id is arbitrary text and `service:org:user#read` is a writable tuple. What makes
/// `org:user` unreachable is not a refusal at the gate — it is that nothing asks. `org:user` is
/// declared without a route gate, so no `service_ap!` call site names it (validation rule 8 and
/// `golden_sql_tests::every_converted_call_site_names_a_permission_that_exists` pin that
/// statically), and its row tier resolves through `sumida_user`, whose service branch reads
/// `service:user` — the namespace that *does* have the gate.
///
/// So the assertion is the one that would actually be a hole if it broke: a grant on the row
/// namespace reaches no row. That is the same defect lab-app's `downloadable_asset` /
/// `lab-app:downloadable_asset` pair had, where two namespaces named one resource and a grant on one
/// did nothing for the other — there it was resolved by honouring both, here by there being only
/// one place to grant.
#[tokio::test]
async fn a_route_gate_opens_exactly_the_namespace_it_names() {
  let db = prepare().await;

  let user = TestUser::create(&db).await.expect("a caller with one grant per namespace");
  let target = TestUser::create(&db).await.expect("somebody to try to reach");

  for namespace in ["user", "trainer:task", "site-app:project", "lab-app:site"] {
    grant(&db, "service", namespace, "read", &user).await;
  }
  let subject = subject_of(&user.model);

  let gate = |namespace: &'static str| {
    let subject = subject.clone();
    let db = db.clone();
    async move {
      Authz::check("service", namespace, "can_read", &subject, &db).await.expect("runs")
    }
  };

  let cases = vec![
    Case::new("`user` — sumida's", true, gate("user").await),
    Case::new("`trainer:task` — trainer's", true, gate("trainer:task").await),
    Case::new("`site-app:project` — site-app's", true, gate("site-app:project").await),
    Case::new("`lab-app:site` — lab-app's", true, gate("lab-app:site").await),
    Case::new("`role` — declared and gated, granted to nobody", false, gate("role").await),
  ];

  report("one route gate per namespace", &cases);

  // `org:user` is the row namespace, and the inventory says so. If it ever grows a route gate the
  // claim below stops being about anything, so the shape is asserted before the behaviour.
  let registry = Authz::registry().expect("the rule set must validate");
  let entry = registry.namespace("org:user").expect("`org:user` is in the inventory");
  assert!(!entry.gated, "`org:user` grew a route gate — the case below is now testing nothing");
  assert_eq!(entry.definition.as_deref(), Some("sumida_user"));

  // Every service-tier grant that can be spelled on the row namespace, written. None of them is
  // refused — `service` object ids are free text — and none of them reaches a user.
  for relation in ["read", "op_read", "operator"] {
    grant(&db, "service", "org:user", relation, &user).await;
  }
  assert!(
    !Authz::check("sumida_user", target.model.id, "view", &subject, &db).await.expect("runs"),
    "a `service:org:user#…` grant reached a user row. `sumida_user#view` reads `service:user`, so \
     the row namespace must not be a second place to grant the same power — that is the two-\
     namespaces-one-resource defect, and it silently doubles the surface of every user grant"
  );
}

// ─── 2. delegation ───────────────────────────────────────────────────────────────────────────

/// `delegation_scope:<scope>#assert` matches its object id by equality and by nothing else.
///
/// A grant on `svc:site-app` must not let the holder assert `svc:lab-app`, and must not let them assert
/// a prefix or an extension of their own scope either. `resolve_owner_key` derives the scope in
/// Rust and compares for equality; a rule that matched by prefix would be a cross-tenant hole that
/// only shows up when somebody registers a scope whose name starts with somebody else's.
#[tokio::test]
async fn a_delegation_scope_matches_by_equality_and_not_by_prefix() {
  let db = prepare().await;

  let holder = TestUser::create(&db).await.expect("delegation holder");
  let outsider = TestUser::create(&db).await.expect("no delegation");

  grant(&db, "delegation_scope", "svc:site-app", "operator", &holder).await;

  let asserts = |user: &TestUser, scope: &'static str| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move {
      Authz::check("delegation_scope", scope, "assert", &subject, &db).await.expect("runs")
    }
  };

  let cases = vec![
    Case::new("holder asserts its own scope", true, asserts(&holder, "svc:site-app").await),
    Case::new("holder asserts another service's scope", false, asserts(&holder, "svc:lab-app").await),
    Case::new("holder asserts a longer scope", false, asserts(&holder, "svc:site-app:group:x").await),
    Case::new("holder asserts a prefix of its scope", false, asserts(&holder, "svc").await),
    Case::new("outsider asserts the scope", false, asserts(&outsider, "svc:site-app").await),
  ];

  report("delegation", &cases);
}

// ─── 3. the organization hierarchy ───────────────────────────────────────────────────────────

/// `UserService::update`'s organization branch is `sumida_user#edit`, and it reaches **down**.
///
/// The old shape was `ap::ns("org:user")` joined to the user row and ORed with a recursive CTE over
/// `organizations.parent_id`, evaluated per row inside a LATERAL. The rule is one join against
/// `org_closure`, and what has to survive is the direction: a manager of an organization reaches
/// everyone beneath it, and nobody above it or beside it.
///
/// The tree is three deep on purpose. A walk that stops after one hop, or runs the wrong way, gives
/// the same answer as a correct one on a depth-1 fixture — which is how an inverted hierarchy ships.
#[tokio::test]
async fn an_organization_manager_reaches_the_subtree_and_nothing_else() {
  let db = prepare().await;

  let tree = OrgTree::create(&db).await.expect("a three-deep organization tree");
  let manager = Member::create(&db, tree.subject).await.expect("manager of the subject org");

  // `{update}` on `org:user` was the only action any call site used, and `sumida_user#edit` reads
  // `org->manage_user`, so the manager tier is the faithful target. The backfill has no branch for
  // these rows: choosing between the viewer, editor and manager tier is a judgement about what
  // `{update}` meant and not a mechanical rewrite, so it is made here, in the open.
  tuple(
    &db,
    "organization",
    &tree.subject.to_string(),
    "user_manager",
    "user",
    &manager.user.model.id.to_string(),
    "",
  )
  .await;

  let subject = subject_of(&manager.user.model);
  let targets = [
    ("same org", tree.subject, true),
    ("child org", tree.child, true),
    ("grandchild org, two levels down", tree.grandchild, true),
    ("parent org", tree.parent, false),
    ("sibling org", tree.sibling, false),
    ("unrelated branch", tree.aunt, false),
  ];

  let mut cases = Vec::new();
  for (label, org, expected) in targets {
    let target = Member::create(&db, org).await.expect("a target user");
    let actual =
      Authz::check("sumida_user", target.user.model.id, "edit", &subject, &db).await.expect("runs");
    cases.push(Case::new(format!("edits a user in the {label}"), expected, actual));
  }

  report("organization hierarchy (`org:user` → `sumida_user#edit`)", &cases);
}

/// The same rule, as a **list filter** rather than a point check.
///
/// This is the property the design is built around: the compiled rule is a `WHERE` predicate, so
/// the same rule that answers a point check also scopes a paginated list — and `managed_org_ids`'s
/// defect (a policy that passes a single-row check and fails a list check, because the two were
/// separate implementations) is structurally impossible.
#[tokio::test]
async fn the_point_check_and_the_list_filter_agree() {
  let db = prepare().await;

  let tree = OrgTree::create(&db).await.expect("tree");
  let manager = Member::create(&db, tree.subject).await.expect("manager");
  tuple(
    &db,
    "organization",
    &tree.subject.to_string(),
    "user_manager",
    "user",
    &manager.user.model.id.to_string(),
    "",
  )
  .await;

  let inside = Member::create(&db, tree.grandchild).await.expect("a user two levels down");
  let outside = Member::create(&db, tree.aunt).await.expect("a user in another branch");

  let subject = subject_of(&manager.user.model);
  let listed: Vec<i32> = Authz::filter("sumida_user", "edit", &subject, Users::find())
    .expect("compiles")
    .all(&db)
    .await
    .expect("runs")
    .into_iter()
    .map(|user| user.id)
    .collect();

  let mut cases = Vec::new();
  for (label, target, expected) in
    [("inside the subtree", &inside, true), ("outside it", &outside, false)]
  {
    let by_list = listed.contains(&target.user.model.id);
    let by_check = Authz::check("sumida_user", target.user.model.id, "edit", &subject, &db)
      .await
      .expect("runs");
    assert_eq!(
      by_list, by_check,
      "the list and the point check disagree about a user {label} — one rule, two answers"
    );
    cases.push(Case::new(format!("a user {label}"), expected, by_check));
  }

  report("list filter vs point check", &cases);
}

// ─── 4. the board allow/deny composition ─────────────────────────────────────────────────────

/// `authorize_post_mutation` is `board_post#edit`: an intersection inside, an exclusion outside.
///
/// ```text
/// allowed = (group_allow && board_allow) || owner_allow
/// denied  = group_deny  || board_deny   || owner_deny
/// result  = allowed && !denied
/// ```
///
/// Two properties are under test and they pull in opposite directions, which is why both have to be
/// here. The **intersection** must survive — a group grant alone is not enough — and the
/// **exclusion** must bind outside the whole union, so authorship cannot walk through a block.
/// v1 of the design proposed converting the intersection to a union; this is the case that would
/// have caught that.
#[tokio::test]
async fn a_posts_edit_intersects_its_board_and_excludes_a_block() {
  let db = prepare().await;

  let author = TestUser::create(&db).await.expect("the post's author");
  let operator = TestUser::create(&db).await.expect("an operator with group and board grants");
  let group_only = TestUser::create(&db).await.expect("a holder of the group grant alone");

  let group_id = BoardGroups::insert(entity::bps::board_groups::ActiveModel {
    name: Set(format!("eq-group-{}", api::util::random::generate_url_safe_string(6).unwrap())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("board group")
  .last_insert_id;

  let board_id = Boards::insert(entity::bps::boards::ActiveModel {
    group_id: Set(Some(group_id)),
    name: Set(format!("eq-board-{}", api::util::random::generate_url_safe_string(6).unwrap())),
    is_enabled: Set(true),
    max_file_size: Set(0),
    max_file_count: Set(0),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("board")
  .last_insert_id;

  let post_id = BoardPosts::insert(entity::bps::board_posts::ActiveModel {
    board_id: Set(board_id),
    user_id: Set(Some(author.model.id)),
    title: Set("composition".to_owned()),
    content_type: Set(entity::bps::board_posts::BoardPostContentType::Html),
    content: Set(Some("<p>x</p>".to_owned())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("post")
  .last_insert_id;

  // The author gets **nothing**. Authorship derives from `board_posts.user_id` now (§1.5's first
  // widening), where it used to be an `allow` row `create` wrote at `board_post/mod.rs:448` — so a
  // grant here would hide exactly the property the first case asserts.
  grant(&db, "board_group", &group_id.to_string(), "editor", &operator).await;
  grant(&db, "board", &board_id.to_string(), "editor", &operator).await;
  grant(&db, "board_group", &group_id.to_string(), "editor", &group_only).await;

  let edits = |user: &TestUser| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move { Authz::check("board_post", post_id, "edit", &subject, &db).await.expect("runs") }
  };

  let mut cases = vec![
    Case::new("the author, with no grant at all", true, edits(&author).await).noting(
      "§1.5(1), the intended widening: `board_post#author` derives from `board_posts.user_id`, so \
       authorship is no longer a row `create` has to remember to write",
    ),
    Case::new("an operator holding both halves", true, edits(&operator).await),
    Case::new("the group grant alone", false, edits(&group_only).await).noting(
      "the intersection: `board#edit` needs `editor_or_own_here` on the board itself, so reaching \
       it through the group alone is not enough",
    ),
  ];

  // Block the author at board level. The exclusion is subtracted outside the whole union, so it has
  // to beat authorship — this is `pin_a_block_is_not_circumvented_by_authorship` asserted through
  // the rule rather than through HTTP.
  grant(&db, "board", &board_id.to_string(), "blocked", &author).await;
  cases.push(
    Case::new("the author, once blocked on the board", false, edits(&author).await).noting(
      "a `deny` row had no automatic translation — the backfill refuses `effect='deny'` rather \
       than guessing which relation it meant, so this block is §9.2's operator action",
    ),
  );

  report("board allow/deny composition", &cases);
}

// ─── the mirror trigger ──────────────────────────────────────────────────────────────────────

/// The mirror and the backfill must classify a row the same way.
///
/// They are two implementations of one mapping — the backfill is a set-based statement, the mirror
/// a row trigger — and a mapping that disagrees with itself leaves a live database holding grants
/// its own migration would not have written. This writes one policy of each interesting shape, lets
/// the trigger mirror it, and asserts the tuple store holds exactly what the backfill's dispositions
/// say it should.
///
/// It outlives the evaluator it was written beside: the mirror is migration machinery, and
/// `access_policies` is still the table a not-yet-migrated database arrives with.
///
/// One subject per shape, so "this shape produces nothing" can be asserted as "this subject holds
/// no mirrored tuple at all" rather than as the absence of one guessed row.
#[tokio::test]
async fn the_mirror_writes_what_the_backfill_would_have() {
  let db = prepare().await;

  // `Some(relation …)` is what the backfill would have written; `None` is a disposition of
  // `drop:` or `refuse:`, which must leave the tuple store untouched.
  //
  // One legacy policy row, and the tuples it should turn into.
  type Shape = (&'static str, &'static str, &'static [&'static str], &'static str, Option<Mapped>);
  /// `(resource_type, resource_id, relations)`.
  type Mapped = (&'static str, &'static str, &'static [&'static str]);

  let shapes: [Shape; 9] = [
    (
      "user",
      "service",
      &["read", "update"],
      "allow",
      Some(("service", "user", &["read", "update"])),
    ),
    // The route gate written with a wildcard action. It means "every route action on this
    // namespace" and the object is `service`, so there is no object-wide power to guess at — it
    // expands to the four gate relations. A generic wildcard refusal placed before this branch
    // would drop the grant instead, which is §0.2's mistake in a second place.
    (
      "user",
      "service",
      &["*"],
      "allow",
      Some(("service", "user", &["create", "delete", "read", "update"])),
    ),
    (
      "trainer:delegate",
      "svc:site-app",
      &["assert"],
      "allow",
      Some(("delegation_scope", "svc:site-app", &["operator"])),
    ),
    // lab-app's sharing screen writes `{'*'}` for level Edit. A rule that refused wildcards first
    // would drop every share anyone has ever made on that screen.
    ("lab-app:site", "37", &["*"], "allow", Some(("daqs_site", "37", &["editor"]))),
    (
      "site-app:project.row",
      "my-desk",
      &["read", "update", "owner"],
      "allow",
      Some(("antares_project", "my-desk", &["owner"])),
    ),
    // The exclusion mapping assumes there is no per-tuple deny to translate.
    ("user", "service", &["delete"], "deny", None),
    // A wildcard object needs a person to decide between `operator` and `platform:root#admin`.
    ("user", "*", &["read"], "allow", None),
    // Seeded by site-app's initiator; gated on by nothing.
    ("user_log", "service", &["read"], "allow", None),
    // Ownership derives from `board_posts.user_id` in the compiled model; keeping both would create
    // the second copy the design exists to remove.
    ("board_post", "4242", &["update"], "allow", None),
  ];

  let mut cases = Vec::new();
  for (namespace, object, actions, effect, expected) in shapes {
    let user = TestUser::create(&db).await.expect("a subject of its own");
    let subject = format!("user:{}", user.model.id);
    legacy_policy(
      &db,
      &user,
      namespace,
      object,
      actions.iter().map(|a| (*a).to_owned()).collect(),
      effect,
    ).await;

    let observed = mirrored_tuples(&db, &subject).await;
    let wanted: Vec<String> = match expected {
      Some((resource_type, resource_id, relations)) => relations
        .iter()
        .map(|relation| format!("{resource_type}:{resource_id}#{relation}"))
        .collect(),
      None => Vec::new(),
    };

    cases.push(Case::new(
      format!("`{namespace} | {object} | {{{}}} | {effect}`", actions.join(",")),
      true,
      wanted == observed,
    ));
    assert_eq!(
      observed,
      wanted,
      "the mirror disagreed with the backfill's disposition for \
       `{namespace} | {object} | {{{}}} | {effect}`",
      actions.join(",")
    );
  }

  // A revoked policy must take its tuple with it, or the tuple store keeps a grant the operator
  // believes they removed — which is the failure the non-unique `idx_access_policies__tuple`
  // already makes possible today.
  let revoked = TestUser::create(&db).await.expect("a subject to revoke");
  let subject = format!("user:{}", revoked.model.id);
  legacy_policy(&db, &revoked, "user", "service", vec!["read".into()], "allow").await;
  assert_eq!(mirrored_tuples(&db, &subject).await, vec!["service:user#read".to_owned()]);

  entity::access_policies::Entity::delete_many()
    .filter(entity::access_policies::Column::User.eq(subject.clone()))
    .exec(&db)
    .await
    .expect("revoke");

  let after = mirrored_tuples(&db, &subject).await;
  cases.push(Case::new("the policy is revoked", true, after.is_empty()));

  report("the access_policies mirror, against the backfill's dispositions", &cases);
}

// ─── the client's payload ────────────────────────────────────────────────────────────────────

/// `/me/access-policies` still answers what the CASL client has always been given.
///
/// The whole payload is projected from `relation_tuples` now. The client is not being changed in
/// this phase, so the wire shape is a contract: `updateAbilityFor`
/// (`casl/src/runtime/plugin.ts:19`) iterates `action × object` and emits
/// `can(action, `${namespace}:${object}`)`, with a special case on `namespace === "*"`. A missing
/// `"service"` object string, a `namespace` that is no longer the legacy string, or a lost `"*"`
/// entry each silently disable part of the UI — the sort of break that shows up as "the button is
/// gone" days later and not as a failing request.
///
/// **The object tier is deliberately gone from the payload**, and that is the one intended change.
/// It used to be passed through from `access_policies` because that was still what the server
/// resolved row-level access against; it is not, so passing it through would report grants the
/// server never reads. The assertion below is written in that direction — a legacy row is written
/// and must **not** appear — so a regression that quietly restored the passthrough goes red.
#[tokio::test]
async fn me_access_policies_keeps_its_payload() {
  let db = prepare().await;

  let user = TestUser::create(&db).await.expect("a caller with grants");
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  // The route gate, as a tuple, and an object-tier `access_policies` row that must not surface.
  grant(&db, "service", "user", "read", &user).await;
  legacy_policy(&db, &user, "board", "7", vec!["update".into()], "allow").await;

  let mock = Mock::new().await;

  let session = Session::login(mock.server.addr, &user, "a caller with grants").await;
  let body = session.probe_one("/me/access-policies", &"policies").await;
  let policies = body.body().as_array().cloned().unwrap_or_default();

  let entry = |namespace: &str, object: &str| {
    policies.iter().find(|policy| {
      policy["namespace"] == namespace
        && policy["object"].as_array().is_some_and(|objects| objects.iter().any(|o| o == object))
    })
  };

  let gate = entry("user", "service").unwrap_or_else(|| panic!("the route gate is gone: {policies:?}"));
  assert_eq!(gate["effect"], "allow");
  assert!(
    gate["action"].as_array().is_some_and(|actions| actions.iter().any(|a| a == "read")),
    "the route-gate entry lost its action: {gate:?}"
  );

  assert!(
    entry("board", "7").is_none(),
    "an object-tier `access_policies` row reached the payload. The server does not consult that \
     table for row-level access any more, so the client would be told it may edit board 7 and the \
     request would be refused: {policies:?}"
  );

  // A non-admin must not be handed the catch-all, or the client grants `manage all` to everyone.
  assert!(
    entry("*", "*").is_none(),
    "a non-admin received the `*` catch-all: {policies:?}"
  );

  // And an administrator must still get it, from `platform:root#superuser` rather than from the
  // legacy row — this is the branch `plugin.ts:27` turns into `can("manage", "all")`.
  let admin_session = Session::login(mock.server.addr, &admin, "an administrator").await;
  let admin_body = admin_session.probe_one("/me/access-policies", &"policies").await;
  let admin_policies = admin_body.body().as_array().cloned().unwrap_or_default();
  assert!(
    admin_policies.iter().any(|policy| {
      policy["namespace"] == "*"
        && policy["action"].as_array().is_some_and(|actions| actions.iter().any(|a| a == "*"))
    }),
    "an administrator did not receive the `*` entry, so the client loses `manage all`: \
     {admin_policies:?}"
  );
}

/// Every tuple the mirror wrote for one subject, as `type:id#relation`, sorted.
async fn mirrored_tuples(db: &DatabaseConnection, subject_key: &str) -> Vec<String> {
  #[derive(FromQueryResult)]
  struct Row {
    tuple: String,
  }

  let mut rows = Row::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "SELECT resource_type || ':' || resource_id || '#' || relation AS \"tuple\"
       FROM relation_tuples WHERE subject_key = $1 AND created_by = 'access_policies'",
    [subject_key.into()],
  ))
  .all(db)
  .await
  .expect("runs")
  .into_iter()
  .map(|row| row.tuple)
  .collect::<Vec<_>>();
  rows.sort();
  rows
}

// ─── 5. the object tier, per converted call site ─────────────────────────────────────────────

/// The **row-level** tier, over the four mechanisms §14 step 13's conversion actually moves.
///
/// The service-tier case above is about one grant shape; this is about the tier that decides *which
/// rows*, which is what the 33 converted call sites ask. Each case names the answer the old model
/// gave, or — for the three §1.5 differences — says which way it moved and why. Writing those three
/// down rather than deleting them is the point: a difference nobody wrote down is a difference
/// nobody decided.
#[tokio::test]
async fn the_object_tier_answers_per_converted_call_site() {
  let db = prepare().await;

  let holder = TestUser::create(&db).await.expect("a caller holding a per-row grant");
  let author = TestUser::create(&db).await.expect("the author of a post and the owner of a file");
  let gated = TestUser::create(&db).await.expect("a caller holding only the route gate");
  let nobody = TestUser::create(&db).await.expect("a caller holding nothing");
  let operator = TestUser::create(&db).await.expect("a caller holding object-wide read");

  let group_id = BoardGroups::insert(entity::bps::board_groups::ActiveModel {
    name: Set(format!("obj-g-{}", api::util::random::generate_url_safe_string(6).unwrap())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("board group")
  .last_insert_id;

  let board_id = Boards::insert(entity::bps::boards::ActiveModel {
    group_id: Set(Some(group_id)),
    name: Set(format!("obj-b-{}", api::util::random::generate_url_safe_string(6).unwrap())),
    is_enabled: Set(true),
    max_file_size: Set(0),
    max_file_count: Set(0),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("board")
  .last_insert_id;

  let post_id = BoardPosts::insert(entity::bps::board_posts::ActiveModel {
    board_id: Set(board_id),
    user_id: Set(Some(author.model.id)),
    title: Set("object tier".to_owned()),
    content_type: Set(entity::bps::board_posts::BoardPostContentType::Html),
    content: Set(Some("<p>x</p>".to_owned())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("post")
  .last_insert_id;

  let file_uuid = uuid::Uuid::new_v4();
  Files::insert(entity::fs::files::ActiveModel {
    uuid: Set(file_uuid),
    user_id: Set(Some(author.model.id)),
    original_name: Set("obj.txt".to_owned()),
    dir: Set("/obj".to_owned()),
    name: Set(format!("{file_uuid}.txt")),
    mime_type: Set("text/plain".to_owned()),
    size: Set(1),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("file");

  // 세 층을 각기 다른 사람에게 준다: 행 하나(`board#viewer`), 라우트 게이트(`service:board#read`),
  // 그리고 종전 `object='*'` 에 해당하는 객체 전역(`service:file#op_read`).
  grant(&db, "board", &board_id.to_string(), "viewer", &holder).await;
  grant(&db, "service", "board", "read", &gated).await;
  grant(&db, "service", "file", "op_read", &operator).await;

  let views = |definition: &'static str, id: String, user: &TestUser| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move {
      let key = Authz::object_key(definition, &id).expect("a well-typed object id");
      Authz::check(definition, key, "view", &subject, &db).await.expect("runs")
    }
  };

  let cases = vec![
    // (1)(2) 는 서로를 지탱한다. (1) 만 있으면 "전부 통과" 로도 통과하고, (2) 만 있으면
    // "전부 차단" 으로도 통과한다.
    Case::new(
      "a per-row grant on a board reaches that board",
      true,
      views("board", board_id.to_string(), &holder).await,
    ),
    Case::new(
      "a caller with no grant reaches nothing",
      false,
      views("board", board_id.to_string(), &nobody).await,
    ),
    // (3) 설계 §5.2 가 두 층을 갈라 놓은 자리. 종전에도 행 검사에는 `object='service'` 가
    // 맞지 않았고, 지금도 아니다.
    Case::new(
      "a route-gate grant does not reach a row",
      false,
      views("board", board_id.to_string(), &gated).await,
    ),
    // (4) `object='*'` — `file` 처럼 정의가 `service:<ns>->operate_read` 를 품은 쪽은 정확히 옮겨진다.
    Case::new(
      "an object-wide grant on `file` reaches somebody else's file",
      true,
      views("file", file_uuid.to_string(), &operator).await,
    ),
    // (5) R3 — 소유권이 열에서 파생된다. 종전에는 업로드/작성 때 심어 둔 행이 있어야 닿았다.
    Case::new(
      "the author of a post reaches it with no grant at all",
      true,
      views("board_post", post_id.to_string(), &author).await,
    )
    .noting(
      "§1.5(1), the intended widening: `board_post#author` derives from `board_posts.user_id`",
    ),
    Case::new(
      "the owner of a file reaches it with no grant at all",
      true,
      views("file", file_uuid.to_string(), &author).await,
    )
    .noting("§1.5(1) again, for `file#owner` over `files.user_id`"),
    // (6) `board` 에는 객체 전역 읽기의 대응물이 **없다**. 좁힘이며, 규칙 쪽 결정이라 이 단계에서
    // 임의로 메우지 않았다. `operator` 는 위에서 `file` 에만 받았으므로 `board` 에는 아무것도 없다.
    Case::new(
      "an object-wide read grant on `board` has no equivalent",
      false,
      views("board", board_id.to_string(), &operator).await,
    )
    .noting(
      "a narrowing with no rule behind it: `board#view` unions no `service:board->operate_read`, \
       so \"read every board\" is only expressible as ownership. Reported rather than papered \
       over — adding the branch is a rule decision, not a call-site one",
    ),
  ];

  report("the object tier, per converted call site", &cases);
}

// ─── 6. lab-app's object tier ───────────────────────────────────────────────────────────────────

/// The lab-app tables the rules read, in a **schema** of their own.
///
/// Of their own because `Authz::boot` decides a product is deployed by its marker table, and
/// lab-app's marker is `sites`. Creating it in `public` would make every lab-app definition switch from
/// `Absent` to `Drift` for every other test in this binary — the rules read eight more tables —
/// and `boot_reports_which_rules_this_database_can_back` asserts the opposite. A schema is the
/// isolation available here: `live_columns` looks at `current_schema()`, and the compiled
/// predicate names tables unqualified, so a connection whose `search_path` is
/// `daqs_fixture, public` resolves `sites` here and `users`/`relation_tuples` in `public` — while
/// every other connection in the binary sees no lab-app table at all.
///
/// (A separate database would be cleaner still and is not available: the test role has no
/// `CREATEDB`.)
///
/// Only the columns the rules name are here. A fixture that reproduced lab-app's real schema would be
/// a copy that drifts; a fixture that has exactly the columns `schema.lab-app.zed` reads fails loudly
/// the day a rule starts reading one more.
const DAQS_FIXTURE_DDL: &str = r#"
DROP SCHEMA IF EXISTS daqs_fixture CASCADE;
CREATE SCHEMA daqs_fixture;

CREATE TABLE daqs_fixture.sites (
  id         SERIAL PRIMARY KEY,
  manager_id INTEGER NULL REFERENCES public.users(id) ON DELETE SET NULL
);
CREATE TABLE daqs_fixture.structures (
  id      SERIAL PRIMARY KEY,
  site_id INTEGER NOT NULL REFERENCES daqs_fixture.sites(id) ON DELETE CASCADE
);
CREATE TABLE daqs_fixture.photos (
  id           SERIAL PRIMARY KEY,
  structure_id INTEGER NOT NULL REFERENCES daqs_fixture.structures(id) ON DELETE CASCADE
);
CREATE TABLE daqs_fixture.inspections (
  id       SERIAL PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES daqs_fixture.photos(id) ON DELETE CASCADE
);
CREATE TABLE daqs_fixture.reports (
  id      SERIAL PRIMARY KEY,
  site_id INTEGER NOT NULL REFERENCES daqs_fixture.sites(id) ON DELETE CASCADE
);
CREATE TABLE daqs_fixture.ai_models (id SERIAL PRIMARY KEY);
CREATE TABLE daqs_fixture.downloadable_assets (id SERIAL PRIMARY KEY);
CREATE TABLE daqs_fixture.inquiries (
  post_id INTEGER PRIMARY KEY REFERENCES public.board_posts(id) ON DELETE CASCADE
);
CREATE TABLE daqs_fixture.inquiry_ratings (
  post_id INTEGER PRIMARY KEY REFERENCES public.board_posts(id) ON DELETE CASCADE
);

-- One id band per table, and they do not overlap.
--
-- Every walk under test carries an id from one table into another (`photos.structure_id` becomes a
-- site id, `inspections.photo_id` becomes a photo id). With all five sequences starting at 1, a
-- rule that walked one hop too few would read structure 3 as site 3 — and on a small fixture those
-- are the same row often enough that a wrong rule passes. Disjoint bands make that impossible: an
-- id from the wrong table names nothing at all.
ALTER SEQUENCE daqs_fixture.sites_id_seq       RESTART WITH 1000;
ALTER SEQUENCE daqs_fixture.structures_id_seq  RESTART WITH 2000;
ALTER SEQUENCE daqs_fixture.photos_id_seq      RESTART WITH 3000;
ALTER SEQUENCE daqs_fixture.inspections_id_seq RESTART WITH 4000;
ALTER SEQUENCE daqs_fixture.reports_id_seq     RESTART WITH 5000;
"#;

static DAQS_FIXTURE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

fn db_backend() -> sea_orm::DbBackend {
  sea_orm::DbBackend::Postgres
}

/// A connection that sees `daqs_fixture` first and `public` behind it, with the fixture built once
/// per test binary.
///
/// Rebuilt rather than reused (`DROP SCHEMA … CASCADE`): a fixture that survives between runs is a
/// fixture whose state nobody can state, and this file's whole claim is about what the rules say
/// over *known* rows.
async fn prepare_daqs() -> DatabaseConnection {
  // `public` must be migrated first — the fixture's foreign keys point into it.
  let shared = prepare().await;

  let mut options = sea_orm::ConnectOptions::new(std::env::var("DATABASE_URL").expect("DATABASE_URL"));
  options.set_schema_search_path("daqs_fixture,public");
  let db = sea_orm::Database::connect(options)
    .await
    .expect("connect with the fixture schema first");

  DAQS_FIXTURE
    .get_or_init(|| async {
      shared.execute_unprepared(DAQS_FIXTURE_DDL).await.expect("lab-app's tables");
    })
    .await;

  db
}

/// Inserts a row and returns its id, for the fixture tables above.
async fn insert_id(db: &DatabaseConnection, sql: &str) -> i32 {
  #[derive(FromQueryResult)]
  struct Id {
    id: i32,
  }

  Id::find_by_statement(Statement::from_string(db_backend(), sql.to_owned()))
    .one(db)
    .await
    .unwrap_or_else(|e| panic!("{sql}: {e}"))
    .expect("RETURNING id")
    .id
}

/// The site ids `daqs_site#view` selects, read through the **filter** predicate rather than the
/// point check — the same rule, in the other of its two shapes.
async fn visible_site_ids(db: &DatabaseConnection, user: &entity::users::Model) -> Vec<i32> {
  #[derive(FromQueryResult)]
  struct Id {
    id: i32,
  }

  let predicate =
    Authz::predicate_sql("daqs_site", "view", &subject_of(user)).expect("the rule must compile");

  Id::find_by_statement(Statement::from_string(
    db_backend(),
    format!(r#"SELECT "sites"."id" FROM "sites" WHERE {predicate} ORDER BY "sites"."id""#),
  ))
  .all(db)
  .await
  .expect("the filter predicate must be one runnable statement")
  .into_iter()
  .map(|row| row.id)
  .collect()
}

/// lab-app's two sharing levels grant what the screen says they grant.
///
/// `SitePermissionLevel::Read` is `viewer` and `Edit` is `editor` — the mapping that replaced the
/// old `{read}` / `{'*'}` action arrays, and the reason the wildcard-action problem disappeared
/// rather than being handled. What has to hold is that the two levels stay **different**: a Read
/// share must not carry `edit` or `remove`, or the screen's two options collapse into one.
#[tokio::test]
async fn daqs_sharing_levels_grant_different_permissions() {
  let db = prepare_daqs().await;

  let reader = TestUser::create(&db).await.expect("a caller with a Read share");
  let editor = TestUser::create(&db).await.expect("a caller with an Edit share");
  let stranger = TestUser::create(&db).await.expect("a caller with no share at all");

  let read_site = insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await;
  let edit_site = insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await;

  grant(&db, "daqs_site", &read_site.to_string(), "viewer", &reader).await;
  grant(&db, "daqs_site", &edit_site.to_string(), "editor", &editor).await;

  let ask = |user: &TestUser, site: i32, permission: &'static str, expected: bool| {
    let subject = subject_of(&user.model);
    let username = user.model.username.clone();
    let db = db.clone();
    async move {
      let actual =
        Authz::check("daqs_site", site, permission, &subject, &db).await.expect("the rule must run");
      Case::new(format!("{username} on site {site} — {permission}"), expected, actual)
    }
  };

  let cases = vec![
    ask(&reader, read_site, "view", true).await,
    ask(&reader, read_site, "edit", false).await,
    ask(&reader, read_site, "remove", false).await,
    ask(&editor, edit_site, "view", true).await,
    ask(&editor, edit_site, "edit", true).await,
    ask(&editor, edit_site, "create", true).await,
    ask(&editor, edit_site, "remove", true).await,
    // The share is on one site and must not reach the other, in either direction.
    ask(&reader, edit_site, "view", false).await,
    ask(&editor, read_site, "view", false).await,
    ask(&stranger, read_site, "view", false).await,
    ask(&stranger, edit_site, "edit", false).await,
  ];

  report("lab-app:site sharing levels (Read = viewer, Edit = editor)", &cases);

  // And the same rule as a list filter selects exactly the sites its point check passes — the
  // property that makes "the list showed it but the detail page refused" impossible to write.
  assert_eq!(
    visible_site_ids(&db, &reader.model).await,
    vec![read_site],
    "the Read share's list filter and its point check disagree"
  );
  assert_eq!(
    visible_site_ids(&db, &editor.model).await,
    vec![edit_site],
    "the Edit share's list filter and its point check disagree"
  );
  assert!(
    visible_site_ids(&db, &stranger.model).await.is_empty(),
    "a caller with no share saw a site through the list filter"
  );
}

/// An organization share reaches the organization's members, and nobody else.
///
/// `org:<id>` meant "any member of this organization" without ever saying so; the rule says it, as
/// the userset `organization:<id>#member`. Writing it as a bare `organization:<id>` subject would
/// grant nothing at all — a failure that looks exactly like a permission problem in production — so
/// the negative half (a member of a *different* organization) is here too.
#[tokio::test]
async fn a_daqs_site_shared_to_an_organization_reaches_its_members() {
  let db = prepare_daqs().await;

  let org = TestOrg::create(&db).await.expect("the organization holding the share");
  let member = TestUser::create_in_org(&db, org.model.id).await.expect("a member of it");
  let outsider = TestUser::create(&db).await.expect("a member of some other organization");

  let site = insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await;

  tuple(
    &db,
    "daqs_site",
    &site.to_string(),
    "viewer",
    "organization",
    &org.model.id.to_string(),
    "member",
  )
  .await;

  let ask = |user: &entity::users::Model, what: &str, expected: bool| {
    let subject = subject_of(user);
    let what = what.to_owned();
    let db = db.clone();
    async move {
      let actual =
        Authz::check("daqs_site", site, "view", &subject, &db).await.expect("the rule must run");
      Case::new(what, expected, actual)
    }
  };

  report(
    "lab-app:site organization share (`org:N` → `organization:N#member`)",
    &[
      ask(&member.model, "a member of the sharing organization", true).await,
      ask(&outsider.model, "a member of another organization", false).await,
    ],
  );

  assert_eq!(
    visible_site_ids(&db, &member.model).await,
    vec![site],
    "the organization share did not reach its member through the list filter"
  );
  assert!(
    visible_site_ids(&db, &outsider.model).await.is_empty(),
    "the organization share reached somebody outside the organization"
  );
}

/// The four walks — structure, photo, inspection, report — reach the site and stop there.
///
/// Each was a hand-assembled join at the call site: `lab-app:site` joined on `structures.site_id`, on
/// `photos.structure_id → structures.site_id`, and on
/// `inspections.photo_id → photos.structure_id → structures.site_id`. They are `site->view` on four
/// definitions now, and the joins that remain in the services are for the projection.
///
/// The fixture's disjoint id bands are what make this test able to fail: with every sequence
/// starting at 1 a rule that walked one hop too few would read structure 3 as site 3, and pass.
#[tokio::test]
async fn daqs_child_rows_walk_to_their_site_and_stop() {
  let db = prepare_daqs().await;

  let sharer = TestUser::create(&db).await.expect("a caller with a share on one site");
  let stranger = TestUser::create(&db).await.expect("a caller with none");

  let shared = insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await;
  let other = insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await;
  grant(&db, "daqs_site", &shared.to_string(), "editor", &sharer).await;

  let row = |table: &str, column: &str, parent: i32| {
    let sql = format!("INSERT INTO {table} ({column}) VALUES ({parent}) RETURNING id");
    let db = db.clone();
    async move { insert_id(&db, &sql).await }
  };

  let shared_structure = row("structures", "site_id", shared).await;
  let other_structure = row("structures", "site_id", other).await;
  let shared_photo = row("photos", "structure_id", shared_structure).await;
  let other_photo = row("photos", "structure_id", other_structure).await;
  let shared_inspection = row("inspections", "photo_id", shared_photo).await;
  let other_inspection = row("inspections", "photo_id", other_photo).await;
  let shared_report = row("reports", "site_id", shared).await;
  let other_report = row("reports", "site_id", other).await;

  let walk = |user: &TestUser, definition: &'static str, id: i32, site: i32, expected: bool| {
    let subject = subject_of(&user.model);
    let username = user.model.username.clone();
    let db = db.clone();
    async move {
      let actual =
        Authz::check(definition, id, "view", &subject, &db).await.expect("the rule must run");
      Case::new(format!("{username}: {definition}:{id} → site {site}"), expected, actual)
    }
  };

  report(
    "lab-app's walks to the site",
    &[
      walk(&sharer, "daqs_structure", shared_structure, shared, true).await,
      walk(&sharer, "daqs_structure", other_structure, other, false).await,
      walk(&sharer, "daqs_photo", shared_photo, shared, true).await,
      walk(&sharer, "daqs_photo", other_photo, other, false).await,
      walk(&sharer, "daqs_inspection", shared_inspection, shared, true).await,
      walk(&sharer, "daqs_inspection", other_inspection, other, false).await,
      walk(&sharer, "daqs_report", shared_report, shared, true).await,
      walk(&sharer, "daqs_report", other_report, other, false).await,
      walk(&stranger, "daqs_photo", shared_photo, shared, false).await,
      walk(&stranger, "daqs_inspection", shared_inspection, shared, false).await,
    ],
  );
}

/// The two things lab-app's rules changed on purpose, asserted as changes.
///
/// * **The manager column is an ownership edge.** `daqs_site#manager` derives from
///   `sites.manager_id`, which the old evaluator could not see at all — it only ever saw the
///   `allow ["*"]` policy row `ensure_manager_policy` wrote *alongside* the column. No live grant
///   moves, because lab-app still writes that row (as an `editor` tuple now); what changes is that a
///   site whose row was lost and whose column survived stops being unreachable by its own manager.
/// * **`blocked` binds outside the whole union.** A block on a caller who also holds a share must
///   win, and it must win on `view` and not only on `edit` — `daqs_site#edit` subtracts `blocked`
///   on its own, so a case built on an Edit share would still pass with the outer subtraction on
///   `view` deleted. The share here is deliberately **Read**.
#[tokio::test]
async fn daqs_site_reads_its_manager_column_and_honours_a_block() {
  let db = prepare_daqs().await;

  let manager = TestUser::create(&db).await.expect("the site's manager");
  let blocked = TestUser::create(&db).await.expect("a caller with a share and a block");

  let managed = insert_id(
    &db,
    &format!("INSERT INTO sites (manager_id) VALUES ({}) RETURNING id", manager.model.id),
  )
  .await;
  let denied = insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await;

  grant(&db, "daqs_site", &denied.to_string(), "viewer", &blocked).await;

  let views = |user: &TestUser, site: i32| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move {
      Authz::check("daqs_site", site, "view", &subject, &db).await.expect("the rule must run")
    }
  };

  let before = views(&blocked, denied).await;
  grant(&db, "daqs_site", &denied.to_string(), "blocked", &blocked).await;

  report(
    "lab-app:site — the manager column and the block",
    &[
      Case::new("the manager, by column alone, with no tuple", true, views(&manager, managed).await)
        .noting(
          "`manager` derives from `sites.manager_id`; the old evaluator only ever saw the row lab-app \
           writes beside it, so the column alone reached nobody",
        ),
      Case::new("a Read share, before the block", true, before),
      Case::new("the same share, once blocked", false, views(&blocked, denied).await).noting(
        "a `deny` row had no automatic translation — the backfill refuses `effect='deny'`, so \
         this is §9.2's operator action performed in the open",
      ),
      // The manager case is worthless unless the managed site is unreachable without the column.
      Case::new(
        "the managed site, to a caller who is not its manager and holds no share",
        false,
        views(&blocked, managed).await,
      ),
    ],
  );
}

/// lab-app's inquiry screens are boards, and the rules say so.
///
/// Both services authorized through `ap::ns("board")` joined on `board_posts.board_id`; the
/// definitions walk `inquiries.post_id → board_posts.board_id → board`. The property under test is
/// that the walk lands on the *post's own* board — a rule that resolved the board any other way
/// would let an inquiry on one board be read through a grant on another.
#[tokio::test]
async fn daqs_inquiries_walk_to_their_own_board() {
  let db = prepare_daqs().await;

  let user = TestUser::create(&db).await.expect("a caller with a grant on one board");

  let granted = insert_id(&db, "INSERT INTO boards (name) VALUES ('granted') RETURNING id").await;
  let other = insert_id(&db, "INSERT INTO boards (name) VALUES ('other') RETURNING id").await;

  let post = |board: i32| {
    let sql = format!(
      "INSERT INTO board_posts (board_id, user_id, title, content, content_type) \
       VALUES ({board}, {}, 'q', 'q', 'text') RETURNING id",
      user.model.id
    );
    let db = db.clone();
    async move { insert_id(&db, &sql).await }
  };

  let granted_post = post(granted).await;
  let other_post = post(other).await;

  for post_id in [granted_post, other_post] {
    for table in ["inquiries", "inquiry_ratings"] {
      db.execute(Statement::from_string(
        db_backend(),
        format!("INSERT INTO {table} (post_id) VALUES ({post_id})"),
      ))
      .await
      .expect("the inquiry row");
    }
  }

  grant(&db, "board", &granted.to_string(), "viewer", &user).await;

  let ask = |definition: &'static str, post_id: i32, board: i32, expected: bool| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move {
      let actual =
        Authz::check(definition, post_id, "view", &subject, &db).await.expect("the rule must run");
      Case::new(format!("{definition}:{post_id} on board {board}"), expected, actual)
    }
  };

  report(
    "lab-app's inquiries walk to the board",
    &[
      ask("daqs_inquiry", granted_post, granted, true).await,
      ask("daqs_inquiry", other_post, other, false).await,
      ask("daqs_inquiry_rating", granted_post, granted, true).await,
      ask("daqs_inquiry_rating", other_post, other, false).await,
    ],
  );
}

/// lab-app's site list has a second filter beside the rule, and it was raw SQL against
/// `access_policies`. This is the question it asked, asked of `relation_tuples`.
///
/// ```sql
/// -- what it was (lab-app `site/mod.rs`, before §14 step 13)
/// CAST("sites"."id" AS text) IN (
///   SELECT ap."object" FROM "access_policies" ap
///   LEFT JOIN "users" u ON ap."user" = CONCAT('user:', u."id")
///   WHERE ap."namespace" = 'lab-app:site' AND ap."effect" = 'allow'
///     AND (ap."user" = CONCAT('org:', CAST($1 AS text)) OR u."organization_id" = $1)
/// )
/// ```
///
/// Read plainly: **which sites are assigned to organization N** — where "assigned" is either a
/// share held by the organization itself, or a share held by any one of its people. It is not a
/// permission check; `daqs_site#view` has already decided what the caller may see, and this narrows
/// that list to one organization for the screen's select box.
///
/// This is the one place a legacy statement still runs, and it is the whole point of the case: both
/// texts are executed against the same seeded data and their answers compared. The seed is written
/// as `access_policies` rows so the mirror produces the tuples, which is what makes the two sides
/// comparable at all rather than two hand-written fixtures that agree because they were written to.
///
/// Three things change in the translation and none of them is the question: `effect = 'allow'`
/// becomes the relation list (`deny` is a rule subtraction now, not a row, so it was never in
/// scope); `org:N` becomes the userset `organization:N#member`; and expiry is honoured, which the
/// old table could not express at all.
///
/// **The seam this test does not close.** The `TRANSLATED` text below is a copy of lab-app's
/// `SITE_ORGANIZATION_FILTER`, because sumida cannot depend on lab-app — the dependency runs the other
/// way. `lab-app/server/src/lab-app/service/site/mod.rs`'s own unit test pins that constant against the
/// same text, so an edit on either side goes red on one of the two; an edit to both, with no edit
/// here, is the hole. It is named rather than papered over.
const LEGACY_SITE_ORGANIZATION_FILTER: &str = r#"CAST("sites"."id" AS text) IN (
  SELECT ap."object"
  FROM "access_policies" ap
  LEFT JOIN "users" u ON ap."user" = CONCAT('user:', u."id")
  WHERE ap."namespace" = 'lab-app:site'
    AND ap."effect" = 'allow'
    AND (ap."user" = CONCAT('org:', CAST($1 AS text)) OR u."organization_id" = $1)
)"#;

const TRANSLATED_SITE_ORGANIZATION_FILTER: &str = r#"CAST("sites"."id" AS text) IN (
  SELECT rt."resource_id"
  FROM "relation_tuples" rt
  LEFT JOIN "users" u ON rt."subject_key" = CONCAT('user:', u."id")
  WHERE rt."resource_type" = 'daqs_site'
    AND rt."relation" IN ('viewer', 'editor')
    AND (rt."expires_at" IS NULL OR rt."expires_at" > CURRENT_TIMESTAMP)
    AND (rt."subject_key" = CONCAT('organization:', CAST($1 AS text), '#member')
         OR u."organization_id" = $1)
)"#;

#[tokio::test]
async fn daqs_site_organization_filter_asks_the_same_question() {
  let db = prepare_daqs().await;

  let alpha = TestOrg::create(&db).await.expect("organization alpha");
  let beta = TestOrg::create(&db).await.expect("organization beta");
  let in_alpha = TestUser::create_in_org(&db, alpha.model.id).await.expect("a person in alpha");
  let in_beta = TestUser::create_in_org(&db, beta.model.id).await.expect("a person in beta");

  let site = || {
    let db = db.clone();
    async move { insert_id(&db, "INSERT INTO sites DEFAULT VALUES RETURNING id").await }
  };

  let org_shared = site().await; // shared to alpha itself
  let person_shared = site().await; // shared to one of alpha's people
  let elsewhere = site().await; // shared to somebody in beta
  let unshared = site().await; // shared to nobody

  // The organization share, which `legacy_policy` cannot write — it always writes a `user:`
  // subject. The mirror translates both shapes.
  entity::access_policies::Entity::insert(entity::access_policies::ActiveModel {
    namespace: Set("lab-app:site".to_owned()),
    object:    Set(org_shared.to_string()),
    action:    Set(vec!["read".to_owned()]),
    effect:    Set("allow".to_owned()),
    user:      Set(format!("org:{}", alpha.model.id)),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("the organization share");

  legacy_policy(&db, &in_alpha, "lab-app:site", &person_shared.to_string(), vec!["*".into()], "allow").await;
  legacy_policy(&db, &in_beta, "lab-app:site", &elsewhere.to_string(), vec!["read".into()], "allow").await;

  #[derive(FromQueryResult)]
  struct Id {
    id: i32,
  }

  let assigned = |filter: &'static str, organization_id: i32| {
    let db = db.clone();
    async move {
      let sql = format!(
        r#"SELECT "sites"."id" FROM "sites" WHERE {} ORDER BY "sites"."id""#,
        filter.replace("$1", &organization_id.to_string())
      );
      Id::find_by_statement(Statement::from_string(db_backend(), sql.clone()))
        .all(&db)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<_>>()
    }
  };

  let mut expected = vec![org_shared, person_shared];
  expected.sort_unstable();

  for (organization, label) in [(alpha.model.id, "alpha"), (beta.model.id, "beta")] {
    let legacy = assigned(LEGACY_SITE_ORGANIZATION_FILTER, organization).await;
    let translated = assigned(TRANSLATED_SITE_ORGANIZATION_FILTER, organization).await;

    println!("\n### the site→organization assignment filter, for {label}\n");
    println!("| source | sites |");
    println!("|---|---|");
    println!("| access_policies (was) | {legacy:?} |");
    println!("| relation_tuples (is)  | {translated:?} |");
    println!();

    assert_eq!(
      legacy, translated,
      "the translated organization filter selects different sites for {label} — it is asking a \
       different question than the raw SQL it replaced"
    );
  }

  // And the answer is the right one, not merely the same one on both sides: a filter that returned
  // nothing at all would also be "equal".
  assert_eq!(
    assigned(TRANSLATED_SITE_ORGANIZATION_FILTER, alpha.model.id).await,
    expected,
    "the organization filter did not find both the organization share and its member's share"
  );
  assert_eq!(
    assigned(TRANSLATED_SITE_ORGANIZATION_FILTER, beta.model.id).await,
    vec![elsewhere],
    "beta's assignment leaked or went missing"
  );
  assert!(
    !assigned(TRANSLATED_SITE_ORGANIZATION_FILTER, alpha.model.id).await.contains(&unshared),
    "a site nobody shared was reported as assigned"
  );

  // Expiry is the one thing the old table could not say. An expired share must stop counting as an
  // assignment — the property that would silently not exist if the clause were dropped.
  db.execute(Statement::from_string(
    db_backend(),
    format!(
      "UPDATE relation_tuples SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' \
       WHERE resource_type = 'daqs_site' AND resource_id = '{person_shared}'"
    ),
  ))
  .await
  .expect("expire the personal share");

  assert_eq!(
    assigned(TRANSLATED_SITE_ORGANIZATION_FILTER, alpha.model.id).await,
    vec![org_shared],
    "an expired share still counted as an organization assignment"
  );
}
