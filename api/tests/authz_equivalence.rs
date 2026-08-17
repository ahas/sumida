//! Equivalence: does the compiled rule answer what `ap.rs` answers, against a real database?
//!
//! The golden-SQL tests in `api/src/authz/golden_sql_tests.rs` pin what the compiler *emits*. They
//! cannot tell you whether the emitted predicate selects the same rows the current evaluator
//! selects, and that is the only question that matters for a rewrite whose acceptance criterion is
//! "no defect in service usage or in permission setting". So these run both evaluators over the
//! same Postgres, with the same seed data, and compare the answers.
//!
//! Four areas, chosen because each is a different mechanism rather than a different namespace:
//!
//! | area | today | compiled |
//! |---|---|---|
//! | service-tier route gate | `ap::ns(ns).object("service").action(a)` | `service:<ns>#can_<a>` |
//! | delegation | `ap::ns("trainer:delegate").object(scope).action("assert")` | `delegation_scope:<scope>#assert` |
//! | organization hierarchy | `ap::ns("org:user") + organization_ancestors_subquery` | `sumida_user:<id>#edit` via `^` |
//! | board allow/deny composition | `authorize_post_mutation` (`board_post/mod.rs:465-500`) | `board_post:<id>#edit` |
//!
//! Every case prints a row of `legacy | compiled | verdict` and then asserts the two agree. Where
//! they cannot agree without an operator doing something — a `deny` row has no automatic
//! translation, because the backfill deliberately refuses to guess one — the case says so in its
//! note and asserts the agreement *after* the equivalent tuple exists. Silence would be the wrong
//! answer there; so would a passing test that quietly skipped it.
//!
//! Requires a real Postgres. Nothing in the request path is exercised: both evaluators are called
//! directly, so a difference here is a difference in the rules, not in a handler.

use api::authz::subject::SubjectSet;
use api::authz::{Authz, DefinitionState};
use api::util::ap::{self, ApGuard, ApUser};
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

/// One comparison, and what the two evaluators said.
struct Case {
  what:     String,
  legacy:   bool,
  compiled: bool,
  /// Why a difference is expected, when one is. `None` means the two must agree.
  note:     Option<&'static str>,
}

/// Renders the comparison as a table and fails on the first genuine disagreement.
///
/// The table is printed whether or not the assertion holds, because "legacy rows vs compiled rows,
/// identical / differs and why" is the artefact this test exists to produce — a bare `assert_eq!`
/// leaves a reviewer with nothing to read.
fn report(area: &str, cases: &[Case]) {
  report_as(area, "ap.rs", "compiled", cases);
}

fn report_as(area: &str, left: &str, right: &str, cases: &[Case]) {
  println!("\n### {area}\n");
  println!("| case | {left} | {right} | verdict |");
  println!("|---|---|---|---|");
  for case in cases {
    let verdict = match (case.legacy == case.compiled, case.note) {
      (true, _) => "identical".to_owned(),
      (false, Some(note)) => format!("differs — {note}"),
      (false, None) => "**DIVERGES**".to_owned(),
    };
    println!("| {} | {} | {} | {verdict} |", case.what, case.legacy, case.compiled);
  }
  println!();

  for case in cases {
    if case.note.is_none() {
      assert_eq!(
        case.legacy, case.compiled,
        "`{}`: ap.rs said {}, the compiled rule said {}",
        case.what, case.legacy, case.compiled
      );
    }
  }
}

/// The identities `who()` expands a session into (`ap.rs:300`), without needing a session.
fn legacy_who(user: &entity::users::Model) -> Vec<ApUser> {
  let mut users =
    vec![ApUser::User(user.id), ApUser::Role(user.role_id), ApUser::Org(user.organization_id)];
  if user.is_admin {
    users.push(ApUser::Admin);
  }
  users
}

/// The same identity, for the compiled evaluator.
///
/// `is_admin` is deliberately **not** here: R1 is `platform#flagged`, a derived tupleset over
/// `users.is_admin`, so the flag enters through `user_ids` and not through an extra principal.
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

/// A check pinned to the **legacy** evaluator.
///
/// `.without_routing()` is load-bearing, not decoration. Since the Phase 1 shim (§14 step 7),
/// `ap::ns(..).object("service")` answers from the compiled rule for a routed namespace — so a
/// comparison written without this would put the compiled evaluator on both sides and pass no
/// matter what either one does. The whole file would keep reporting "identical" while measuring
/// nothing.
fn legacy(namespace: &str, object: &str, action: &str, user: &entity::users::Model) -> ap::ApCondition {
  let mut condition = ap::ns(namespace).object(object).action(action).without_routing();
  for principal in legacy_who(user) {
    condition = condition.user(principal);
  }
  condition
}

/// Writes one tuple. `Authz::write` — with its authority check and `created_by` stamp — belongs to
/// the phase that introduces the write path; here the point is what the *reader* does with a tuple
/// that exists.
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

/// Brings the vocabulary projection up to date, so a tuple on any relation the rules declare can be
/// written at all. The bootstrap seed in the `rebac` migration covers only what the backfill needs.
async fn prepare() -> DatabaseConnection {
  // Loads `.env.e2e` and wipes/re-migrates the test database, once per binary — the same entry the
  // other suites use. The server itself is never started: both evaluators are called directly.
  e2e::bootstrap::init_once().await;
  let db = infra::db::connect().await;
  // Brings the vocabulary projection up to date, so a tuple on any relation the rules declare can
  // be written at all. The bootstrap seed in the `rebac` migration covers only what the backfill
  // needs.
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

/// `service_ap!(state, "user", read)` compiles to `service:user#can_read`.
///
/// The one deliberate semantic change is asserted here rather than left implicit: today
/// `_build_object_condition` ORs `object = '*'` unconditionally, so an `object='*'` row also
/// satisfies the route gate. In the compiled rule the object-wide grant (`op_read`) still implies
/// the gate, but a gate grant does not imply object-wide power. Both directions are checked.
#[tokio::test]
async fn service_tier_gate_matches() {
  let db = prepare().await;

  let nobody = TestUser::create(&db).await.expect("a user with no grants");
  let gated = TestUser::create(&db).await.expect("a user with the route gate");
  let operator = TestUser::create(&db).await.expect("a user with object-wide power");
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  // The legacy rows. The mirror trigger turns each into the equivalent tuple as it is written,
  // which is what makes this a test of the mirror as well as of the evaluator.
  e2e::ap_test_utils::create_ap(&db, &gated, "user", "service", vec!["read".into()], "allow")
    .await
    .expect("grant the route gate");
  e2e::ap_test_utils::create_ap(&db, &operator, "user", "*", vec!["read".into()], "allow")
    .await
    .expect("grant object-wide read");
  // `object = '*'` is refused by the backfill and by the mirror — it needs a person to decide
  // whether it meant `operator` or `platform:root#admin`. This is that decision, made explicitly.
  tuple(&db, "service", "user", "op_read", "user", &operator.model.id.to_string(), "").await;

  let mut cases = Vec::new();
  for (label, user) in [
    ("no grant", &nobody),
    ("route-gate grant", &gated),
    ("object-wide grant", &operator),
    ("administrator", &admin),
  ] {
    let legacy_allowed = legacy("user", "service", "read", &user.model).check_allowed(&db).await;
    let compiled_allowed = Authz::check("service", "user", "can_read", &subject_of(&user.model), &db)
      .await
      .expect("compiles and runs");

    cases.push(Case {
      what:     format!("`user` read gate, {label}"),
      legacy:   legacy_allowed,
      compiled: compiled_allowed,
      note:     None,
    });
  }

  // The narrowing, stated as a case of its own: a route-gate grant must not confer object-wide
  // power. Today it does not either — `object = 'service'` never matches a row-level check — so
  // this is a property being preserved, not introduced.
  let gate_holder_is_operator =
    Authz::check("service", "user", "operate_read", &subject_of(&gated.model), &db)
      .await
      .expect("compiles and runs");
  cases.push(Case {
    what:     "route-gate grant confers object-wide read".to_owned(),
    legacy:   false,
    compiled: gate_holder_is_operator,
    note:     None,
  });

  report("service-tier route gate", &cases);
}

/// The route gate really is answered from `relation_tuples` — not merely capable of being.
///
/// Every case above would still pass if the shim silently fell back for every namespace, because
/// the mirror keeps both stores holding the same grants and both evaluators would then agree by
/// accident. So this grants a route gate **as a tuple only**, with no `access_policies` row behind
/// it, and asks `ap::ns(..)` — the same call `service_ap!` makes. The legacy evaluator cannot see
/// that grant; if the answer is `true`, the compiled one was consulted.
///
/// The negative half matters as much: `trainer:task` is a real, gated namespace whose product has not
/// opted in (`ROUTED_PRODUCTS`), and the identical tuple must **not** reach it. That is what makes
/// this a test of per-namespace routing rather than of a global flip.
#[tokio::test]
async fn the_route_gate_is_answered_from_tuples() {
  let db = prepare().await;

  let user = TestUser::create(&db).await.expect("a caller whose only grant is a tuple");
  let id = user.model.id.to_string();

  tuple(&db, "service", "user", "read", "user", &id, "").await;
  tuple(&db, "service", "trainer:task", "read", "user", &id, "").await;

  let gate = |namespace: &'static str| {
    let user = user.model.clone();
    let db = db.clone();
    async move {
      let mut condition = ap::ns(namespace).object("service").action("read");
      for principal in legacy_who(&user) {
        condition = condition.user(principal);
      }
      condition.check_allowed(&db).await
    }
  };

  let routed = gate("user").await;
  let unrouted = gate("trainer:task").await;

  report(
    "per-namespace routing (tuple-only grant, no policy row)",
    &[
      Case {
        what:     "`user` — sumida's, routed".to_owned(),
        legacy:   false,
        compiled: routed,
        note:     Some(
          "the tuple is invisible to `access_policies`, so the two must differ — that difference \
           *is* the routing",
        ),
      },
      Case {
        what:     "`trainer:task` — trainer has not opted in".to_owned(),
        legacy:   false,
        compiled: unrouted,
        note:     None,
      },
    ],
  );

  assert!(
    routed,
    "a tuple-only grant did not open `user`'s route gate — the shim fell back to access_policies \
     for a namespace that is supposed to route, and every other case in this file agrees by \
     accident rather than by construction"
  );
  assert!(
    !unrouted,
    "the identical tuple opened `trainer:task`'s gate — routing is global, not per-namespace, and \
     trainer's Phase 2 has just happened by accident"
  );
}

// ─── 2. delegation ───────────────────────────────────────────────────────────────────────────

/// `ap::ns("trainer:delegate").object(scope).action("assert")` compiles to
/// `delegation_scope:<scope>#assert`.
///
/// The property worth the test is exact matching: a grant on `svc:site-app` must not let the holder
/// assert `svc:lab-app`, and must not let them assert a *prefix* or *extension* of their own scope
/// either. `resolve_owner_key` derives the scope in Rust and compares the object for equality;
/// nothing about that changes, but a rule that matched by prefix would be a cross-tenant hole.
#[tokio::test]
async fn delegation_matches() {
  let db = prepare().await;

  let holder = TestUser::create(&db).await.expect("delegation holder");
  let outsider = TestUser::create(&db).await.expect("no delegation");

  e2e::ap_test_utils::create_ap(
    &db,
    &holder,
    "trainer:delegate",
    "svc:site-app",
    vec!["assert".into()],
    "allow",
  )
  .await
  .expect("grant the delegation");

  let mut cases = Vec::new();
  for (label, user, scope) in [
    ("holder asserts its own scope", &holder, "svc:site-app"),
    ("holder asserts another service's scope", &holder, "svc:lab-app"),
    ("holder asserts a longer scope", &holder, "svc:site-app:group:x"),
    ("outsider asserts the scope", &outsider, "svc:site-app"),
  ] {
    let legacy_allowed = legacy("trainer:delegate", scope, "assert", &user.model).check_allowed(&db).await;
    let compiled_allowed = Authz::check("delegation_scope", scope, "assert", &subject_of(&user.model), &db)
      .await
      .expect("compiles and runs");

    cases.push(Case {
      what:     label.to_owned(),
      legacy:   legacy_allowed,
      compiled: compiled_allowed,
      note:     None,
    });
  }

  report("delegation", &cases);
}

// ─── 3. the organization hierarchy ───────────────────────────────────────────────────────────

/// `UserService::update`'s organization branch (`user/mod.rs:111`) compiles to `sumida_user#edit`.
///
/// Today it is `ap::ns("org:user")` joined to the user row and ORed with
/// `organization_ancestors_subquery(users.organization_id)` — a recursive CTE evaluated per row
/// inside the LATERAL, whose own doc warns the cost accumulates by N×depth. The compiled rule is
/// one join against `org_closure`, and this asserts the two select the same people.
///
/// The tree is three deep on purpose. A walk that stops after one hop, or runs the wrong way, gives
/// the same answer as a correct one on a depth-1 fixture — which is how an inverted hierarchy ships.
#[tokio::test]
async fn organization_hierarchy_matches() {
  let db = prepare().await;

  let tree = OrgTree::create(&db).await.expect("a three-deep organization tree");
  let manager = Member::create(&db, tree.subject).await.expect("manager of the subject org");

  // The legacy grant: object is the organization id, matched against each target's ancestor chain.
  e2e::ap_test_utils::create_ap(
    &db,
    &manager.user,
    "org:user",
    &tree.subject.to_string(),
    vec!["update".into()],
    "allow",
  )
  .await
  .expect("grant org-tier update");

  // The compiled equivalent. `org:user` rows are **not** mirrored automatically: the backfill has
  // no branch for them, because deciding between the viewer, editor and manager tier is a judgement
  // about what `{update}` meant and not a mechanical rewrite. `{update}` on `org:user` is the only
  // action any call site uses, and `sumida_user#edit` reads `org->manage_user`, so the manager tier
  // is the faithful target.
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

  let targets = [
    ("same org", tree.subject),
    ("child org", tree.child),
    ("grandchild org, two levels down", tree.grandchild),
    ("parent org", tree.parent),
    ("sibling org", tree.sibling),
    ("unrelated branch", tree.aunt),
  ];

  let mut cases = Vec::new();
  for (label, org) in targets {
    let target = Member::create(&db, org).await.expect("a target user");

    let start =
      sea_orm::sea_query::Expr::col((entity::users::Entity, entity::users::Column::OrganizationId));
    let legacy_allowed = {
      let mut condition = ap::ns("org:user")
        .action("update")
        .join(Users, entity::users::Column::Id)
        .object_in_subquery(api::util::org::organization_ancestors_subquery(
          sea_orm::IntoSimpleExpr::into_simple_expr(start),
        ));
      for principal in legacy_who(&manager.user.model) {
        condition = condition.user(principal);
      }
      condition.can_access(Users::find_by_id(target.user.model.id), &db).await
    };

    let compiled_allowed =
      Authz::check("sumida_user", target.user.model.id, "edit", &subject_of(&manager.user.model), &db)
        .await
        .expect("compiles and runs");

    cases.push(Case {
      what:     format!("manager of the subject org edits a user in the {label}"),
      legacy:   legacy_allowed,
      compiled: compiled_allowed,
      note:     None,
    });
  }

  report("organization hierarchy (`org:user` → `sumida_user#edit`)", &cases);
}

/// The same rule, as a **list filter** rather than a point check.
///
/// This is the property the design is built around: the compiled rule is a `WHERE` predicate, so
/// the same rule that answers a point check also scopes a paginated list — and `managed_org_ids`'s
/// defect (a policy that passes a single-row check and fails a list check, because the two are
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
  for (label, target) in [("inside the subtree", &inside), ("outside it", &outside)] {
    let by_list = listed.contains(&target.user.model.id);
    let by_check = Authz::check("sumida_user", target.user.model.id, "edit", &subject, &db)
      .await
      .expect("runs");
    cases.push(Case {
      what:     format!("a user {label}"),
      legacy:   by_list,
      compiled: by_check,
      note:     None,
    });
  }

  report("list filter vs point check (both compiled)", &cases);

  assert!(listed.contains(&inside.user.model.id), "the list must reach two levels down");
  assert!(!listed.contains(&outside.user.model.id), "and must not leave the subtree");
}

// ─── 4. the board allow/deny composition ─────────────────────────────────────────────────────

/// `authorize_post_mutation` (`board_post/mod.rs:465-500`) compiles to `board_post#edit`.
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
///
/// The `deny` row itself has no automatic translation. The backfill refuses `effect='deny'` rather
/// than guessing, so the compiled side gets an explicit `blocked` tuple — which is the operator
/// action §9.2 describes, performed here in the open rather than assumed away.
#[tokio::test]
async fn board_allow_deny_composition_matches() {
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
    title: Set("equivalence".to_owned()),
    content_type: Set(entity::bps::board_posts::BoardPostContentType::Html),
    content: Set(Some("<p>x</p>".to_owned())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("post")
  .last_insert_id;

  // Today's ownership grant, written by `create` at `board_post/mod.rs:448`. In the compiled model
  // authorship derives from `board_posts.user_id`, which is why the mirror deliberately drops this
  // row — keeping both would create the second copy the design exists to remove.
  e2e::ap_test_utils::create_ap(
    &db,
    &author,
    "board_post",
    &post_id.to_string(),
    vec!["update".into(), "delete".into()],
    "allow",
  )
  .await
  .expect("grant authorship");

  // The operator holds both halves of the intersection; `group_only` holds one.
  for (user, namespace, object) in [
    (&operator, "board_group", group_id.to_string()),
    (&operator, "board", board_id.to_string()),
    (&group_only, "board_group", group_id.to_string()),
  ] {
    e2e::ap_test_utils::create_ap(&db, user, namespace, &object, vec!["update".into()], "allow")
      .await
      .expect("grant");
  }
  // Neither `board_group` nor `board` object-tier rows are mirrored (their objects are row ids, a
  // shape the backfill has no branch for), so the compiled side gets the equivalent tuples.
  for (user, resource_type, resource_id) in [
    (&operator, "board_group", group_id.to_string()),
    (&operator, "board", board_id.to_string()),
    (&group_only, "board_group", group_id.to_string()),
  ] {
    tuple(&db, resource_type, &resource_id, "editor", "user", &user.model.id.to_string(), "").await;
  }

  let post_edit = |user: &entity::users::Model| {
    let user = user.clone();
    let db = db.clone();
    async move {
      // `authorize_post_mutation`, transcribed. Written out rather than called because the method
      // is private to the service and takes a session; the arithmetic is the thing under test.
      let mut group = ap::ns("board_group").object(group_id).action("update");
      let mut board = ap::ns("board").object(board_id).action("update");
      let mut owner = ap::ns("board_post").object(post_id).action("update");
      for principal in legacy_who(&user) {
        group = group.user(principal.clone());
        board = board.user(principal.clone());
        owner = owner.user(principal);
      }
      let (group_allow, group_deny) = group.effects(&db).await;
      let (board_allow, board_deny) = board.effects(&db).await;
      let (owner_allow, owner_deny) = owner.effects(&db).await;

      let allowed = (group_allow && board_allow) || owner_allow;
      let denied = group_deny || board_deny || owner_deny;
      allowed && !denied
    }
  };

  let mut cases = Vec::new();
  for (label, user) in [
    ("the author", &author),
    ("an operator with both grants", &operator),
    ("the group grant alone", &group_only),
  ] {
    let legacy_allowed = post_edit(&user.model).await;
    let compiled_allowed = Authz::check("board_post", post_id, "edit", &subject_of(&user.model), &db)
      .await
      .expect("compiles and runs");
    cases.push(Case {
      what:     format!("{label} edits the post"),
      legacy:   legacy_allowed,
      compiled: compiled_allowed,
      note:     None,
    });
  }

  // Now block the author at board level. `who()` puts `user:<id>` on the session, so the deny row
  // names the author directly — this is the case `pin_a_block_is_not_circumvented_by_authorship`
  // asserts through HTTP.
  e2e::ap_test_utils::create_ap(
    &db,
    &author,
    "board",
    &board_id.to_string(),
    vec!["update".into()],
    "deny",
  )
  .await
  .expect("block the author at board level");

  let legacy_after_block = post_edit(&author.model).await;
  let compiled_before_tuple = Authz::check("board_post", post_id, "edit", &subject_of(&author.model), &db)
    .await
    .expect("runs");
  cases.push(Case {
    what:     "the author, after a board-level deny row, before the block is expressed as a tuple"
      .to_owned(),
    legacy:   legacy_after_block,
    compiled: compiled_before_tuple,
    note:     Some(
      "a `deny` row has no automatic translation: the backfill refuses `effect='deny'` rather than \
       guessing which relation it meant, so the block does not exist on the compiled side yet",
    ),
  });

  // The operator action §9.2 describes: the deny becomes a `blocked` tuple.
  tuple(&db, "board", &board_id.to_string(), "blocked", "user", &author.model.id.to_string(), "").await;

  let compiled_after_tuple = Authz::check("board_post", post_id, "edit", &subject_of(&author.model), &db)
    .await
    .expect("runs");
  cases.push(Case {
    what:     "the author, after the block is expressed as `board#blocked`".to_owned(),
    legacy:   legacy_after_block,
    compiled: compiled_after_tuple,
    note:     None,
  });

  report("board allow/deny composition", &cases);

  assert!(
    !compiled_after_tuple,
    "a board-level block was circumvented by post authorship — the absolute-deny property is gone"
  );
  assert!(
    compiled_before_tuple,
    "the case is worthless unless authorship really did permit the edit before the block"
  );
}

// ─── the mirror trigger ──────────────────────────────────────────────────────────────────────

/// The mirror and the backfill must classify a row the same way.
///
/// They are two implementations of one mapping — the backfill is a set-based statement, the mirror
/// a row trigger — and a mapping that disagrees with itself is worse than no mirror at all, because
/// the shadow comparison would then be measuring the disagreement rather than the migration. This
/// writes one policy of each interesting shape, lets the trigger mirror it, and asserts the tuple
/// store holds exactly what the backfill's dispositions say it should.
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
    e2e::ap_test_utils::create_ap(
      &db,
      &user,
      namespace,
      object,
      actions.iter().map(|a| (*a).to_owned()).collect(),
      effect,
    )
    .await
    .expect("write the policy");

    let observed = mirrored_tuples(&db, &subject).await;
    let wanted: Vec<String> = match expected {
      Some((resource_type, resource_id, relations)) => relations
        .iter()
        .map(|relation| format!("{resource_type}:{resource_id}#{relation}"))
        .collect(),
      None => Vec::new(),
    };

    cases.push(Case {
      what:     format!("`{namespace} | {object} | {{{}}} | {effect}`", actions.join(",")),
      legacy:   wanted == observed,
      compiled: true,
      note:     None,
    });
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
  e2e::ap_test_utils::create_ap(&db, &revoked, "user", "service", vec!["read".into()], "allow")
    .await
    .expect("grant");
  assert_eq!(mirrored_tuples(&db, &subject).await, vec!["service:user#read".to_owned()]);

  entity::access_policies::Entity::delete_many()
    .filter(entity::access_policies::Column::User.eq(subject.clone()))
    .exec(&db)
    .await
    .expect("revoke");

  let after = mirrored_tuples(&db, &subject).await;
  cases.push(Case {
    what:     "the policy is revoked".to_owned(),
    legacy:   after.is_empty(),
    compiled: true,
    note:     None,
  });

  report_as("the access_policies mirror", "matches the backfill", "expected", &cases);
  assert!(after.is_empty(), "revoking the policy left {after:?} behind");
}

// ─── the client's payload ────────────────────────────────────────────────────────────────────

/// `/me/access-policies` still answers what the CASL client has always been given.
///
/// The route-gate half of that payload is projected from `relation_tuples` now (§9.7). The client
/// is not being changed in this phase, so the wire shape is a contract: `updateAbilityFor`
/// (`casl/src/runtime/plugin.ts:19`) iterates `action × object` and emits
/// `can(action, `${namespace}:${object}`)`, with a special case on `namespace === "*"`. A missing
/// `"service"` object string, a `namespace` that is no longer the legacy string, or a lost `"*"`
/// entry each silently disable part of the UI — the sort of break that shows up as "the button is
/// gone" days later and not as a failing request.
///
/// So this asserts the payload against what the legacy projection produced for the same subject,
/// element for element, rather than against a hand-written expectation.
#[tokio::test]
async fn me_access_policies_keeps_its_payload() {
  let db = prepare().await;

  let user = TestUser::create(&db).await.expect("a caller with grants");
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  // One route-gate grant and one object-tier grant. Both must survive, and they now come from
  // different stores — which is the whole reason this case exists.
  e2e::ap_test_utils::create_ap(&db, &user, "user", "service", vec!["read".into()], "allow")
    .await
    .expect("grant the route gate");
  e2e::ap_test_utils::create_ap(&db, &user, "board", "7", vec!["update".into()], "allow")
    .await
    .expect("grant an object-tier row");

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
    entry("board", "7").is_some(),
    "the object-tier grant vanished from the payload while the server still resolves it from \
     `access_policies` — the client would hide UI whose request succeeds: {policies:?}"
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
