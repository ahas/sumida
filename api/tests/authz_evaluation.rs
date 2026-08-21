//! What the compiled rules answer, against a real database, per mechanism.
//!
//! Every case is an **absolute** claim: the answer the rules must give, written down. Nothing here
//! compares one evaluator against another, so a case that no longer says something is a case to
//! delete rather than to loosen.
//!
//! Six mechanisms, chosen because each is a different one rather than a different namespace:
//!
//! | area | rule |
//! |---|---|
//! | service-tier route gate | `service:<ns>#can_<a>`, and its narrowing against `operate_<a>` |
//! | delegation | `delegation_scope:<scope>#assert`, matched by equality and never by prefix |
//! | organization hierarchy | `sumida_user:<id>#edit` through the `^` closure |
//! | board allow/deny composition | `board_post:<id>#edit` — an intersection inside, an exclusion outside |
//! | the object tier, per call site | `<definition>:<id>#<permission>` |
//! | an installed consumer's object tier | the sharing levels, the walks, and the block |
//!
//! The last row is the fixture rule set, `tests/fixtures/schema.demo.sz`, installed through the
//! same seam a real product uses. sumida ships its own rules and knows of no others, so the shapes
//! a *consumer* writes — a slug key, a canonical subject-key column, a walk three tables deep —
//! have to be installed here or they are untested.
//!
//! Each case prints a row of `expected | actual` and then asserts. The table is kept because it is
//! the artefact a reviewer reads: a bare `assert!` says a rule is wrong without saying which of a
//! dozen shapes it was wrong about. Where the rules answer something a reader would not have
//! predicted — a widening or a narrowing the model chose deliberately — the expectation *is* that
//! answer and the `note` says which way it goes and why, because a decision nobody wrote down is a
//! decision nobody made.
//!
//! The fixture always writes the store the rules read. A test of a rule that seeds through a
//! translation layer goes red for the layer's reasons as well as the rule's.
//!
//! Requires a real Postgres. Nothing in the request path is exercised except
//! [`me_access_policies_keeps_its_payload`]: the evaluator is called directly, so a failure here is
//! a failure in the rules and not in a handler.

use api::authz::registry::RuleFile;
use api::authz::subject::SubjectSet;
use api::authz::{Authz, DefinitionState};
use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult, Statement};

// ─── harness ─────────────────────────────────────────────────────────────────────────────────

/// One claim about the rules, and what they actually said.
struct Case {
  what:     String,
  expected: bool,
  actual:   bool,
  /// Set when the expectation is one a reader would not have predicted. It does not weaken the
  /// assertion — `expected` is still asserted — it says *why* the expectation is what it is.
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
/// `is_admin` is deliberately **not** an extra principal: `platform#flagged` is a derived tupleset
/// over `users.is_admin`, so the flag enters through `user_ids`. This is
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
async fn grant(
  db: &DatabaseConnection,
  resource_type: &str,
  resource_id: &str,
  relation: &str,
  user: &TestUser,
) {
  tuple(db, resource_type, resource_id, relation, "user", &user.model.id.to_string(), "").await;
}

/// The fixture rule set a consumer would install, in the shape a consumer installs it.
const DEMO_RULES: &[RuleFile] = &[RuleFile {
  name: "schema.demo.sz",
  text: include_str!("fixtures/schema.demo.sz"),
}];

/// Installs the fixture rules and brings the vocabulary projection up to date, so a tuple on any
/// relation the rules declare can be written at all.
///
/// The install has to happen before anything reads the registry, which is why every test in this
/// binary goes through here rather than reaching for `infra::db::connect` directly — the same
/// ordering a product's `main` has to observe, exercised by observing it.
async fn prepare() -> DatabaseConnection {
  // **Before** the harness, not after: `init_once` projects the vocabulary, and projecting reads
  // the rule set. Installing below it would install rules the projection had already been written
  // without — which `install_rules` refuses, loudly, for exactly that reason.
  Authz::install_rules(DEMO_RULES).expect("the fixture rules must install before the first read");

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
/// sumida's own definitions must be **active**; the installed fixture's must be **absent**, because
/// its tables live in a schema of their own and are not on this connection's search path. An
/// `Absent` is not a failure — a rule whose table is not here cannot run here, which is also what a
/// switched-off optional feature looks like — but a `Drift` is: it means a rule reads a column of a
/// table that *is* here and does not have it.
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

  for definition in ["demo_ticket", "demo_widget", "demo_root"] {
    assert!(
      matches!(report.state_of(definition), Some(DefinitionState::Absent { .. })),
      "`{definition}`'s tables are not on this connection's search path, and boot said {:?}",
      report.state_of(definition)
    );
  }

  let drifted: Vec<_> = report.drifted().map(|entry| entry.definition.clone()).collect();
  assert!(drifted.is_empty(), "a rule reads a column this database does not have: {drifted:?}");
}

// ─── 1. the service-tier route gate ──────────────────────────────────────────────────────────

/// `service_ap!(state, "user", read)` is `service:user#can_read`, and the two tiers stay apart.
///
/// The implication runs one way only, and both directions are cases of their own: `operate_read`
/// implies `can_read`, and a route-gate grant does **not** imply object-wide power. A rule that
/// collapsed the two would pass a test that checked only the first.
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
  // column *is* the grant and there is no row to write.

  let can_read = |user: &TestUser| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move { Authz::check("service", "user", "can_read", &subject, &db).await.expect("runs") }
  };

  let cases = vec![
    Case::new("no grant reaches the gate", false, can_read(&nobody).await),
    Case::new("a route-gate grant opens the gate", true, can_read(&gated).await),
    Case::new("an object-wide grant also opens the gate", true, can_read(&operator).await),
    Case::new("an administrator holds every gate", true, can_read(&admin).await)
      .noting("from `platform:root#superuser` over `users.is_admin`, not from any granted row"),
    Case::new(
      "a route-gate grant confers object-wide read",
      false,
      Authz::check("service", "user", "operate_read", &subject_of(&gated.model), &db)
        .await
        .expect("runs"),
    )
    .noting("the tiers are separate grants: \"may use the user API\" is not \"may see everyone\""),
  ];

  report("the service-tier route gate", &cases);
}

/// A route gate is an **object id** on `service`, and it opens exactly the one namespace it names.
///
/// There is one store of grants and nothing to fall back to, so what this asserts is one namespace
/// per grant, and no leakage between the products that share this rule set.
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
/// namespace reaches no row. That is the same defect a prefixed/unprefixed namespace pair has, where
/// two namespaces name one resource and a grant on one does nothing for the other — there it is
/// resolved by one definition honouring both, here by there being only one place to grant.
#[tokio::test]
async fn a_route_gate_opens_exactly_the_namespace_it_names() {
  let db = prepare().await;

  let user = TestUser::create(&db).await.expect("a caller with one grant per namespace");
  let target = TestUser::create(&db).await.expect("somebody to try to reach");

  for namespace in ["user", "demo:widget", "demo:ticket", "demo:root"] {
    grant(&db, "service", namespace, "read", &user).await;
  }
  let subject = subject_of(&user.model);

  let gate = |namespace: &'static str| {
    let subject = subject.clone();
    let db = db.clone();
    async move { Authz::check("service", namespace, "can_read", &subject, &db).await.expect("runs") }
  };

  let cases = vec![
    Case::new("`user` — sumida's own", true, gate("user").await),
    Case::new("`demo:widget` — the installed consumer's", true, gate("demo:widget").await),
    Case::new("`demo:ticket` — the installed consumer's", true, gate("demo:ticket").await),
    Case::new("`demo:root` — the installed consumer's", true, gate("demo:root").await),
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
/// A grant on `svc:alpha` must not let the holder assert `svc:beta`, and must not let them assert a
/// prefix or an extension of their own scope either. The scope is derived in Rust and compared for
/// equality; a rule that matched by prefix would be a cross-tenant hole that only shows up when
/// somebody registers a scope whose name starts with somebody else's.
#[tokio::test]
async fn a_delegation_scope_matches_by_equality_and_not_by_prefix() {
  let db = prepare().await;

  let holder = TestUser::create(&db).await.expect("delegation holder");
  let outsider = TestUser::create(&db).await.expect("no delegation");

  grant(&db, "delegation_scope", "svc:alpha", "operator", &holder).await;

  let asserts = |user: &TestUser, scope: &'static str| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move { Authz::check("delegation_scope", scope, "assert", &subject, &db).await.expect("runs") }
  };

  let cases = vec![
    Case::new("holder asserts its own scope", true, asserts(&holder, "svc:alpha").await),
    Case::new("holder asserts another service's scope", false, asserts(&holder, "svc:beta").await),
    Case::new("holder asserts a longer scope", false, asserts(&holder, "svc:alpha:group:x").await),
    Case::new("holder asserts a prefix of its scope", false, asserts(&holder, "svc").await),
    Case::new("outsider asserts the scope", false, asserts(&outsider, "svc:alpha").await),
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
    let actual = Authz::check("sumida_user", target.user.model.id, "edit", &subject, &db)
      .await
      .expect("runs");
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
  for (label, target, expected) in [("inside the subtree", &inside, true), ("outside it", &outside, false)]
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

  // The author gets **nothing**. Authorship derives from `board_posts.user_id`, so a grant here
  // would hide exactly the property the first case asserts.
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
      "`board_post#author` derives from `board_posts.user_id`, so authorship is a column and not a \
       row `create` has to remember to write",
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
  cases.push(Case::new("the author, once blocked on the board", false, edits(&author).await).noting(
    "a block is an operator action: `blocked` is written on the object, and the exclusion is \
       subtracted outside the whole union so authorship cannot walk through it",
  ));

  report("board allow/deny composition", &cases);
}

// ─── the client's payload ────────────────────────────────────────────────────────────────────

/// `/me/access-policies` answers the shape the CASL client consumes.
///
/// The payload is projected from `relation_tuples`, and its wire shape is a contract with a client
/// this repository does not build: `updateAbilityFor` iterates `action × object` and emits
/// `can(action, `${namespace}:${object}`)`, with a special case on `namespace === "*"`. A missing
/// `"service"` object string, a renamed `namespace`, or a lost `"*"` entry each silently disable
/// part of the UI — the sort of break that shows up as "the button is gone" days later and not as
/// a failing request.
///
/// **The payload carries the service tier and nothing below it.** A row-tier grant says which rows a
/// caller reaches, and the server settles that per request against the rules; handing it to the
/// client would put a second copy of the answer somewhere it can go stale, and the client would
/// draw a control for a row the request then refuses. So the case seeds a row grant the rules do
/// read, and asserts it does **not** appear.
#[tokio::test]
async fn me_access_policies_keeps_its_payload() {
  let db = prepare().await;

  let user = TestUser::create(&db).await.expect("a caller with grants");
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  // The route gate, and a row-tier grant that must not surface beside it.
  grant(&db, "service", "user", "read", &user).await;
  grant(&db, "demo_widget", "widget-7", "editor", &user).await;

  let mock = Mock::with_rules(DEMO_RULES).await;

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
    entry("demo_widget", "widget-7").is_none(),
    "a row-tier grant reached the payload. The client would be told it may edit that row, and the \
     answer would then be settled again per request against the rules, which is the one place it \
     is settled: {policies:?}"
  );

  // A non-admin must not be handed the catch-all, or the client grants `manage all` to everyone.
  assert!(entry("*", "*").is_none(), "a non-admin received the `*` catch-all: {policies:?}");

  // An administrator must get it, from `platform:root#superuser` — this is the entry the client
  // turns into `can("manage", "all")`.
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

// ─── 5. the object tier, per call site ───────────────────────────────────────────────────────

/// The **row-level** tier, over the four mechanisms a call site can ask for.
///
/// The service-tier case above is about one grant shape; this is about the tier that decides *which
/// rows*. Each case names the answer the rules must give, and where that answer is one a reader
/// would not have predicted the `note` says which way it goes and why: a decision nobody wrote
/// down is a decision nobody made.
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
  // 객체 전역(`service:file#op_read`).
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
    // (3) 두 층이 갈라지는 자리. 라우트 게이트 부여는 행 검사에 닿지 않는다.
    Case::new(
      "a route-gate grant does not reach a row",
      false,
      views("board", board_id.to_string(), &gated).await,
    ),
    // (4) 객체 전역 층 — `file` 처럼 정의가 `service:<ns>->operate_read` 를 품은 쪽에서만 성립한다.
    Case::new(
      "an object-wide grant on `file` reaches somebody else's file",
      true,
      views("file", file_uuid.to_string(), &operator).await,
    ),
    // (5) 소유권은 열에서 파생된다. 부여할 것도, 작성 때 심어 둘 행도 없다.
    Case::new(
      "the author of a post reaches it with no grant at all",
      true,
      views("board_post", post_id.to_string(), &author).await,
    )
    .noting("`board_post#author` derives from `board_posts.user_id`, so the author needs no grant"),
    Case::new(
      "the owner of a file reaches it with no grant at all",
      true,
      views("file", file_uuid.to_string(), &author).await,
    )
    .noting("the same shape for `file#owner` over `files.user_id`"),
    // (6) `board` 에는 객체 전역 읽기의 대응물이 **없다**. 규칙 쪽 결정이므로 호출 자리에서 임의로
    // 메우지 않는다. `operator` 는 위에서 `file` 에만 받았으므로 `board` 에는 아무것도 없다.
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

// ─── 6. an installed consumer's object tier ──────────────────────────────────────────────────

/// The fixture tables the installed rules read, in a **schema** of their own.
///
/// Of their own because every other test in this binary asserts these definitions are `Absent` —
/// their tables must not be in `public`, or `boot_reports_which_rules_this_database_can_back` would
/// see them switch to `Active` and then to `Drift` the moment the fixture is one column short of
/// what a rule reads.
///
/// A schema is the isolation available here: `live_columns` looks at `current_schema()`, and the
/// compiled predicate names tables unqualified, so a connection whose `search_path` is
/// `demo_fixture, public` resolves `roots` here and `users`/`relation_tuples` in `public` — while
/// every other connection in the binary sees no fixture table at all.
///
/// (A separate database would be cleaner still and is not available: the test role has no
/// `CREATEDB`.)
///
/// Only the columns the rules name are here. A fixture that reproduced a whole product's schema
/// would be a copy that drifts; a fixture that has exactly the columns `schema.demo.sz` reads fails
/// loudly the day a rule starts reading one more.
const DEMO_FIXTURE_DDL: &str = r#"
DROP SCHEMA IF EXISTS demo_fixture CASCADE;
CREATE SCHEMA demo_fixture;

CREATE TABLE demo_fixture.roots (
  id         SERIAL PRIMARY KEY,
  manager_id INTEGER NULL REFERENCES public.users(id) ON DELETE SET NULL
);
CREATE TABLE demo_fixture.branches (
  id      SERIAL PRIMARY KEY,
  root_id INTEGER NOT NULL REFERENCES demo_fixture.roots(id) ON DELETE CASCADE
);
CREATE TABLE demo_fixture.leaves (
  id        SERIAL PRIMARY KEY,
  branch_id INTEGER NOT NULL REFERENCES demo_fixture.branches(id) ON DELETE CASCADE
);
CREATE TABLE demo_fixture.buds (
  id      SERIAL PRIMARY KEY,
  leaf_id INTEGER NOT NULL REFERENCES demo_fixture.leaves(id) ON DELETE CASCADE
);
CREATE TABLE demo_fixture.notes (
  id      SERIAL PRIMARY KEY,
  root_id INTEGER NOT NULL REFERENCES demo_fixture.roots(id) ON DELETE CASCADE
);
CREATE TABLE demo_fixture.assets (id SERIAL PRIMARY KEY);
CREATE TABLE demo_fixture.widgets (
  id              SERIAL PRIMARY KEY,
  slug            TEXT    NOT NULL UNIQUE,
  organization_id INTEGER NULL REFERENCES public.organizations(id) ON DELETE SET NULL,
  owner_user_id   INTEGER NULL REFERENCES public.users(id) ON DELETE SET NULL
);
CREATE TABLE demo_fixture.runs (
  id        SERIAL PRIMARY KEY,
  widget_id INTEGER NULL REFERENCES demo_fixture.widgets(id) ON DELETE CASCADE
);
CREATE TABLE demo_fixture.tickets (
  id            UUID PRIMARY KEY,
  owner_key     TEXT    NULL,
  owner_scope   TEXT    NULL,
  owner_user_id INTEGER NULL REFERENCES public.users(id) ON DELETE SET NULL
);
CREATE TABLE demo_fixture.threads (
  post_id INTEGER PRIMARY KEY REFERENCES public.board_posts(id) ON DELETE CASCADE
);

-- One id band per table in the containment tree, and they do not overlap.
--
-- Every walk under test carries an id from one table into another (`leaves.branch_id` becomes a
-- branch id, `buds.leaf_id` becomes a leaf id). With all five sequences starting at 1, a rule that
-- walked one hop too few would read branch 3 as root 3 — and on a small fixture those are the same
-- row often enough that a wrong rule passes. Disjoint bands make that impossible: an id from the
-- wrong table names nothing at all.
ALTER SEQUENCE demo_fixture.roots_id_seq    RESTART WITH 1000;
ALTER SEQUENCE demo_fixture.branches_id_seq RESTART WITH 2000;
ALTER SEQUENCE demo_fixture.leaves_id_seq   RESTART WITH 3000;
ALTER SEQUENCE demo_fixture.buds_id_seq     RESTART WITH 4000;
ALTER SEQUENCE demo_fixture.notes_id_seq    RESTART WITH 5000;
"#;

static DEMO_FIXTURE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

fn db_backend() -> sea_orm::DbBackend {
  sea_orm::DbBackend::Postgres
}

/// A connection that sees `demo_fixture` first and `public` behind it, with the fixture built once
/// per test binary.
///
/// Rebuilt rather than reused (`DROP SCHEMA … CASCADE`): a fixture that survives between runs is a
/// fixture whose state nobody can state, and this file's whole claim is about what the rules say
/// over *known* rows.
async fn prepare_demo() -> DatabaseConnection {
  // `public` must be migrated first — the fixture's foreign keys point into it.
  let shared = prepare().await;

  let mut options = sea_orm::ConnectOptions::new(std::env::var("DATABASE_URL").expect("DATABASE_URL"));
  options.set_schema_search_path("demo_fixture,public");
  let db = sea_orm::Database::connect(options)
    .await
    .expect("connect with the fixture schema first");

  DEMO_FIXTURE
    .get_or_init(|| async {
      shared.execute_unprepared(DEMO_FIXTURE_DDL).await.expect("the fixture's tables");
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

/// The root ids `demo_root#view` selects, read through the **filter** predicate rather than the
/// point check — the same rule, in the other of its two shapes.
async fn visible_root_ids(db: &DatabaseConnection, user: &entity::users::Model) -> Vec<i32> {
  #[derive(FromQueryResult)]
  struct Id {
    id: i32,
  }

  let predicate =
    Authz::predicate_sql("demo_root", "view", &subject_of(user)).expect("the rule must compile");

  Id::find_by_statement(Statement::from_string(
    db_backend(),
    format!(r#"SELECT "roots"."id" FROM "roots" WHERE {predicate} ORDER BY "roots"."id""#),
  ))
  .all(db)
  .await
  .expect("the filter predicate must be one runnable statement")
  .into_iter()
  .map(|row| row.id)
  .collect()
}

/// Two sharing levels grant what the screen that writes them says they grant.
///
/// `viewer` is Read and `editor` is Edit — the mapping that replaced an action array whose Edit
/// level was written as the wildcard `{'*'}`, and the reason the wildcard-action problem
/// disappeared rather than being handled. What has to hold is that the two levels stay
/// **different**: a Read share must not carry `edit` or `remove`, or the screen's two options
/// collapse into one.
#[tokio::test]
async fn sharing_levels_grant_different_permissions() {
  let db = prepare_demo().await;

  let reader = TestUser::create(&db).await.expect("a caller with a Read share");
  let editor = TestUser::create(&db).await.expect("a caller with an Edit share");
  let stranger = TestUser::create(&db).await.expect("a caller with no share at all");

  let read_root = insert_id(&db, "INSERT INTO roots DEFAULT VALUES RETURNING id").await;
  let edit_root = insert_id(&db, "INSERT INTO roots DEFAULT VALUES RETURNING id").await;

  grant(&db, "demo_root", &read_root.to_string(), "viewer", &reader).await;
  grant(&db, "demo_root", &edit_root.to_string(), "editor", &editor).await;

  let ask = |user: &TestUser, root: i32, permission: &'static str, expected: bool| {
    let subject = subject_of(&user.model);
    let username = user.model.username.clone();
    let db = db.clone();
    async move {
      let actual = Authz::check("demo_root", root, permission, &subject, &db)
        .await
        .expect("the rule must run");
      Case::new(format!("{username} on root {root} — {permission}"), expected, actual)
    }
  };

  let cases = vec![
    ask(&reader, read_root, "view", true).await,
    ask(&reader, read_root, "edit", false).await,
    ask(&reader, read_root, "remove", false).await,
    ask(&editor, edit_root, "view", true).await,
    ask(&editor, edit_root, "edit", true).await,
    ask(&editor, edit_root, "create", true).await,
    ask(&editor, edit_root, "remove", true).await,
    // The share is on one root and must not reach the other, in either direction.
    ask(&reader, edit_root, "view", false).await,
    ask(&editor, read_root, "view", false).await,
    ask(&stranger, read_root, "view", false).await,
    ask(&stranger, edit_root, "edit", false).await,
  ];

  report("sharing levels (Read = viewer, Edit = editor)", &cases);

  // And the same rule as a list filter selects exactly the rows its point check passes — the
  // property that makes "the list showed it but the detail page refused" impossible to write.
  assert_eq!(
    visible_root_ids(&db, &reader.model).await,
    vec![read_root],
    "the Read share's list filter and its point check disagree"
  );
  assert_eq!(
    visible_root_ids(&db, &editor.model).await,
    vec![edit_root],
    "the Edit share's list filter and its point check disagree"
  );
  assert!(
    visible_root_ids(&db, &stranger.model).await.is_empty(),
    "a caller with no share saw a row through the list filter"
  );
}

/// An organization share reaches the organization's members, and nobody else.
///
/// `org:<id>` meant "any member of this organization" without ever saying so; the rule says it, as
/// the userset `organization:<id>#member`. Writing it as a bare `organization:<id>` subject would
/// grant nothing at all — a failure that looks exactly like a permission problem in production — so
/// the negative half (a member of a *different* organization) is here too.
#[tokio::test]
async fn a_row_shared_to_an_organization_reaches_its_members() {
  let db = prepare_demo().await;

  let org = TestOrg::create(&db).await.expect("the organization holding the share");
  let member = TestUser::create_in_org(&db, org.model.id).await.expect("a member of it");
  let outsider = TestUser::create(&db).await.expect("a member of some other organization");

  let root = insert_id(&db, "INSERT INTO roots DEFAULT VALUES RETURNING id").await;

  tuple(
    &db,
    "demo_root",
    &root.to_string(),
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
      let actual = Authz::check("demo_root", root, "view", &subject, &db).await.expect("the rule must run");
      Case::new(what, expected, actual)
    }
  };

  report(
    "organization share (`org:N` → `organization:N#member`)",
    &[
      ask(&member.model, "a member of the sharing organization", true).await,
      ask(&outsider.model, "a member of another organization", false).await,
    ],
  );

  assert_eq!(
    visible_root_ids(&db, &member.model).await,
    vec![root],
    "the organization share did not reach its member through the list filter"
  );
  assert!(
    visible_root_ids(&db, &outsider.model).await.is_empty(),
    "the organization share reached somebody outside the organization"
  );
}

/// The four child definitions — branch, leaf, bud, note — reach the root and stop there.
///
/// Each is the shape a hand-assembled join takes at a call site: joined directly on `branches`, one
/// hop through `branches.root_id`, two hops through `leaves.branch_id → branches.root_id`. They are
/// `root->view` on four definitions instead, and the joins that remain in a service are for the
/// projection.
///
/// The fixture's disjoint id bands are what make this test able to fail: with every sequence
/// starting at 1 a rule that walked one hop too few would read branch 3 as root 3, and pass.
#[tokio::test]
async fn child_rows_walk_to_their_root_and_stop() {
  let db = prepare_demo().await;

  let sharer = TestUser::create(&db).await.expect("a caller with a share on one root");
  let stranger = TestUser::create(&db).await.expect("a caller with none");

  let shared = insert_id(&db, "INSERT INTO roots DEFAULT VALUES RETURNING id").await;
  let other = insert_id(&db, "INSERT INTO roots DEFAULT VALUES RETURNING id").await;
  grant(&db, "demo_root", &shared.to_string(), "editor", &sharer).await;

  let row = |table: &str, column: &str, parent: i32| {
    let sql = format!("INSERT INTO {table} ({column}) VALUES ({parent}) RETURNING id");
    let db = db.clone();
    async move { insert_id(&db, &sql).await }
  };

  let shared_branch = row("branches", "root_id", shared).await;
  let other_branch = row("branches", "root_id", other).await;
  let shared_leaf = row("leaves", "branch_id", shared_branch).await;
  let other_leaf = row("leaves", "branch_id", other_branch).await;
  let shared_bud = row("buds", "leaf_id", shared_leaf).await;
  let other_bud = row("buds", "leaf_id", other_leaf).await;
  let shared_note = row("notes", "root_id", shared).await;
  let other_note = row("notes", "root_id", other).await;

  let walk = |user: &TestUser, definition: &'static str, id: i32, root: i32, expected: bool| {
    let subject = subject_of(&user.model);
    let username = user.model.username.clone();
    let db = db.clone();
    async move {
      let actual = Authz::check(definition, id, "view", &subject, &db).await.expect("the rule must run");
      Case::new(format!("{username}: {definition}:{id} → root {root}"), expected, actual)
    }
  };

  report(
    "the walks to the root",
    &[
      walk(&sharer, "demo_branch", shared_branch, shared, true).await,
      walk(&sharer, "demo_branch", other_branch, other, false).await,
      walk(&sharer, "demo_leaf", shared_leaf, shared, true).await,
      walk(&sharer, "demo_leaf", other_leaf, other, false).await,
      walk(&sharer, "demo_bud", shared_bud, shared, true).await,
      walk(&sharer, "demo_bud", other_bud, other, false).await,
      walk(&sharer, "demo_note", shared_note, shared, true).await,
      walk(&sharer, "demo_note", other_note, other, false).await,
      walk(&stranger, "demo_leaf", shared_leaf, shared, false).await,
      walk(&stranger, "demo_bud", shared_bud, shared, false).await,
    ],
  );
}

/// Two properties that pull in opposite directions, which is why both are here.
///
/// * **A manager column is an ownership edge.** `demo_root#manager` derives from
///   `roots.manager_id`, so the column *is* the grant — no row has to be written beside it and none
///   can drift from it. An evaluator that only ever saw a stored grant would leave a row whose
///   column survived and whose grant was lost unreachable by its own manager.
/// * **`blocked` binds outside the whole union.** A block on a caller who also holds a share must
///   win, and it must win on `view` and not only on `edit` — `demo_root#edit` subtracts `blocked`
///   on its own, so a case built on an Edit share would still pass with the outer subtraction on
///   `view` deleted. The share here is deliberately **Read**.
#[tokio::test]
async fn a_manager_column_is_a_grant_and_a_block_beats_one() {
  let db = prepare_demo().await;

  let manager = TestUser::create(&db).await.expect("the row's manager");
  let blocked = TestUser::create(&db).await.expect("a caller with a share and a block");

  let managed = insert_id(
    &db,
    &format!("INSERT INTO roots (manager_id) VALUES ({}) RETURNING id", manager.model.id),
  )
  .await;
  let denied = insert_id(&db, "INSERT INTO roots DEFAULT VALUES RETURNING id").await;

  grant(&db, "demo_root", &denied.to_string(), "viewer", &blocked).await;

  let views = |user: &TestUser, root: i32| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move { Authz::check("demo_root", root, "view", &subject, &db).await.expect("the rule must run") }
  };

  let before = views(&blocked, denied).await;
  grant(&db, "demo_root", &denied.to_string(), "blocked", &blocked).await;

  report(
    "the manager column and the block",
    &[
      Case::new("the manager, by column alone, with no tuple", true, views(&manager, managed).await)
        .noting("`manager` derives from `roots.manager_id`, so the column is the grant"),
      Case::new("a Read share, before the block", true, before),
      Case::new("the same share, once blocked", false, views(&blocked, denied).await)
        .noting("the exclusion binds outside the whole union, so a share cannot be walked around a block"),
      // The manager case is worthless unless the managed row is unreachable without the column.
      Case::new(
        "the managed row, to a caller who is not its manager and holds no share",
        false,
        views(&blocked, managed).await,
      ),
    ],
  );
}

/// A consumer's definition walks into one of **sumida's**, and lands on the right row.
///
/// `demo_thread` is a board post: `threads.post_id → board_posts.board_id → board`. The property
/// under test is that the walk lands on the *post's own* board — a rule that resolved the board any
/// other way would let a thread on one board be read through a grant on another. This is also the
/// direction that makes a change to sumida's `board` rule reach a consumer's screen, which is why
/// it is asserted here rather than left to the consumer.
#[tokio::test]
async fn a_consumer_definition_walks_into_a_sumida_one() {
  let db = prepare_demo().await;

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
    db.execute(Statement::from_string(
      db_backend(),
      format!("INSERT INTO threads (post_id) VALUES ({post_id})"),
    ))
    .await
    .expect("the thread row");
  }

  grant(&db, "board", &granted.to_string(), "viewer", &user).await;

  let ask = |post_id: i32, board: i32, expected: bool| {
    let subject = subject_of(&user.model);
    let db = db.clone();
    async move {
      let actual = Authz::check("demo_thread", post_id, "view", &subject, &db)
        .await
        .expect("the rule must run");
      Case::new(format!("demo_thread:{post_id} on board {board}"), expected, actual)
    }
  };

  report(
    "a consumer's rows walk to sumida's board",
    &[ask(granted_post, granted, true).await, ask(other_post, other, false).await],
  );
}
