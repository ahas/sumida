//! The twelve fixed criticals of design §12.8, re-asserted **against the compiled evaluator**.
//!
//! Each of them is already pinned against `ap.rs` — in `api/tests/authz_golden.rs` for sumida's
//! four, in trainer's `authz_task_read` / `authz_task_stream` / `authz_terminal_session` for trainer's
//! eight. Those pins prove the fix survived in the code that is authoritative *today*. They say
//! nothing about the code that becomes authoritative after the swap, and a regression that
//! reappears only on the new path would pass every one of them.
//!
//! So this file asserts sumida's four through `Authz`. Failing here is a rule defect; failing in
//! `authz_golden.rs` is a code defect; needing both to change at once is the signal that the swap
//! moved a boundary it should not have.
//!
//! | # | property | here |
//! |---|---|---|
//! | G9  | a non-admin cannot mint a key in another account's name | `sumida_user#edit` |
//! | G10 | a key bound to an admin does not carry administrative power when secured | `platform:root#superuser` |
//! | G11 | `/files/stats` is scoped the way `list` is | one `file#view` predicate, two statements |
//! | G12 | delete authorizes against `remove`, not `view` | two permissions, not two strings |
//!
//! **G1–G8 are trainer's, and their row scopes cannot be asserted against `Authz` yet.** Every one of
//! them turns on `joy_task` / `joy_terminal_session` / `joy_cloud_instance`, whose rules read five
//! columns that do not exist (`tasks.owner_user_id`, `tasks.owner_scope`, `jobs.owner_key`,
//! `agents.organization_id`, `cloud_instances.owner_user_id`) — `Authz::boot` reports all three as
//! drift and `boot_or_abort` refuses to start on them. That refusal is the design's intended order,
//! not an oversight: no rule goes live before the column it reads, and the columns land in trainer's
//! own Phase 2 (§14 step 8).
//!
//! Rather than leave that unsaid, trainer pins it:
//! `trainer/server/tests/rebac_e2e.rs::joy_rules_are_drift_blocked_until_their_columns_land` asserts
//! the drift by name and **goes red the moment the columns arrive**, which is the point at which
//! G1–G8 get their compiled-side assertions. The two trainer decision points that *are* expressible
//! today — `delegation_scope#assert` (I5) and `service:trainer:task#operate_read` (the gate
//! `can_access_all` reads) — are asserted in that same file, against both evaluators.
//!
//! Requires a real Postgres. No request goes through the router: the evaluator is called directly,
//! so a failure here is a rule and not a handler.

use api::authz::Authz;
use api::authz::subject::SubjectSet;
use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait, Statement};
use uuid::Uuid;

// ─── harness ─────────────────────────────────────────────────────────────────────────────────

/// Wipes and re-migrates once per binary, then brings the vocabulary projection up to date so a
/// tuple on any relation the rules declare can be written at all.
async fn prepare() -> DatabaseConnection {
  e2e::bootstrap::init_once().await;
  let db = infra::db::connect().await;
  Authz::sync_schema(&db).await.expect("the projection must match the compiled registry");
  db
}

/// The identity of an interactive session, without needing a session.
///
/// `is_admin` is deliberately absent: R1 is `platform#flagged`, a derived tupleset over
/// `users.is_admin`, so the flag enters through `user_ids` and never as an extra principal.
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

async fn seed_file(db: &DatabaseConnection, owner: &entity::users::Model) -> Uuid {
  let uuid = Uuid::new_v4();
  Files::insert(entity::fs::files::ActiveModel {
    uuid: Set(uuid),
    user_id: Set(Some(owner.id)),
    original_name: Set(format!("regression-{uuid}.txt")),
    dir: Set("/regression".to_owned()),
    name: Set(format!("{uuid}.txt")),
    mime_type: Set("text/plain".to_owned()),
    size: Set(1),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("seed a file");
  uuid
}

// ─── G9 ──────────────────────────────────────────────────────────────────────────────────────

/// **G9** — a non-admin cannot create an API key bound to another account.
///
/// `67857ef` fixed this with a handler branch: `if !is_admin && data.user_id != session.id { … }`
/// (`api_key/mod.rs:92`). A handler branch is a thing the next caller can forget, and the reason it
/// was ever needed is that a key inherits its owner's whole identity on verify — so minting a key in
/// somebody's name is exactly as powerful as editing them.
///
/// §8 makes that equality the rule: the guard becomes `sumida_user:<user_id>#edit`. What this pins
/// is that the power is **not free** — an ordinary account holds it over nobody, not even over
/// itself — and that the two ways to acquire it are the two the design names: the organization tier
/// and R1.
#[tokio::test]
async fn g9_minting_a_key_in_another_name_needs_edit_over_that_account() {
  let db = prepare().await;

  let tree = OrgTree::create(&db).await.expect("a three-deep organization tree");
  let attacker = Member::create(&db, tree.subject).await.expect("a plain member");
  let victim = Member::create(&db, tree.child).await.expect("a member one level down");
  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  let attacker_subject = subject_of(&attacker.user.model);
  let victim_id = victim.user.model.id;

  assert!(
    !Authz::check("sumida_user", victim_id, "edit", &attacker_subject, &db).await.expect("runs"),
    "a plain member holds `sumida_user#edit` over another account — G9 is re-opened"
  );
  // The self case is pinned too, because "refuse everything" would satisfy the line above while
  // breaking the feature. Editing *oneself* is `MeService`, not `sumida_user#edit`; what must hold
  // is that this permission confers no authority over one's own account either, or the guard would
  // be a tautology for every caller.
  assert!(
    !Authz::check("sumida_user", attacker.user.model.id, "edit", &attacker_subject, &db)
      .await
      .expect("runs"),
    "`sumida_user#edit` is granted by merely being that user; the guard would then be vacuous"
  );

  // R1 — the platform administrator. `flagged` reads `users.is_admin`, so this is today's admin
  // bypass expressed as a rule rather than as ~55 direct `session.profile.is_admin` reads.
  assert!(
    Authz::check("sumida_user", victim_id, "edit", &subject_of(&admin.model), &db)
      .await
      .expect("runs"),
    "an administrator lost `sumida_user#edit`, so the admin path is broken rather than tightened"
  );

  // R2 — one tuple over the ancestor organization, and the manager reaches the descendant. This is
  // the grant that today needs one policy row per user id.
  tuple(
    &db,
    "organization",
    &tree.subject.to_string(),
    "user_manager",
    "user",
    &attacker.user.model.id.to_string(),
    "",
  )
  .await;
  assert!(
    Authz::check("sumida_user", victim_id, "edit", &attacker_subject, &db).await.expect("runs"),
    "an organization manager cannot edit a user one level down — R2 does not reach"
  );
}

// ─── G10 / I4 ────────────────────────────────────────────────────────────────────────────────

/// **G10, jointly with I4** — a key bound to an administrator.
///
/// The named bug from the brief: `is_admin` was read straight off the session on ~55 authorization
/// paths, and `verify_api_key` fills the session in with the key owner's profile. A **secured** key
/// — one that is supposed to carry only its own grants — therefore carried its owner's
/// administrative power.
///
/// In the compiled model there is nothing to get wrong, because there is no direct read of the flag
/// on an authorization path at all: R1 is `platform:root#superuser`, whose `flagged` branch matches
/// `users.is_admin` against the subject set's `user_ids`. A secured key's subject set has an empty
/// `user_ids`, so the branch is *statically* false rather than accidentally true.
///
/// Both halves are asserted. The unsecured key must still expand to the owner's full identity —
/// that is `user_guard.rs:168-177`'s documented intent, and a "fix" that closed it would break
/// every service key in use.
#[tokio::test]
async fn g10_a_secured_key_bound_to_an_admin_is_not_a_superuser() {
  let db = prepare().await;

  let admin = TestUser::create_admin(&db).await.expect("an administrator");

  // `SubjectSet::of` produces exactly these two shapes from a session; they are built directly here
  // so the case does not depend on the login path it is meant to outlive.
  let secured = SubjectSet::from_parts(vec!["apikey:deadbeef".to_owned()], Vec::new());
  let unsecured = subject_of(&admin.model);

  assert!(
    !Authz::check("platform", "root", "superuser", &secured, &db).await.expect("runs"),
    "a secured key carried `platform:root#superuser` — the named escalation is re-opened"
  );
  assert!(
    Authz::check("platform", "root", "superuser", &unsecured, &db).await.expect("runs"),
    "an unsecured key stopped inheriting its admin owner's identity, which is a different \
     behaviour change and not the fix"
  );

  // The consequence that matters, on a resource rather than on `platform` itself: the secured key
  // reaches nothing through R1.
  let target = TestUser::create(&db).await.expect("somebody to act on");
  assert!(
    !Authz::check("sumida_user", target.model.id, "edit", &secured, &db).await.expect("runs"),
    "a secured key reached `sumida_user#edit` through its owner's admin flag"
  );

  // And the same key with its own grant does work — otherwise "secured keys reach nothing" would
  // pass this file while making the feature useless.
  tuple(&db, "sumida_user", &target.model.id.to_string(), "manager", "apikey", "deadbeef", "").await;
  assert!(
    Authz::check("sumida_user", target.model.id, "edit", &secured, &db).await.expect("runs"),
    "a secured key cannot use a grant made directly to it — `apikey:` subjects are being dropped, \
     which is the defect `membership/mod.rs:170` already has"
  );
}

// ─── G11 ─────────────────────────────────────────────────────────────────────────────────────

/// **G11** — `GET /files/stats` is scoped the way `list` is.
///
/// The aggregate ran over every tenant's files while its sibling `list` went through the scoped
/// select. An aggregate is a disclosure: it answers "how much does that tenant have" without
/// returning a row, and a scope applied to one of the two is a scope that is not applied.
///
/// The compiled rule makes the two the same object — `Authz::filter` adds a `WHERE` predicate and
/// touches nothing else, so the identical predicate serves `SELECT *` and `SELECT count(*)`. This
/// asserts that equality over a corpus rather than asserting it about the source: the count and the
/// listed rows must agree for every caller, including one whose grant reaches only some of them.
#[tokio::test]
async fn g11_the_file_aggregate_and_the_file_list_are_one_predicate() {
  let db = prepare().await;

  let owner = TestUser::create(&db).await.expect("the files' owner");
  let outsider = TestUser::create(&db).await.expect("a caller with no file grants");

  let mine = seed_file(&db, &owner.model).await;
  let _theirs = seed_file(&db, &owner.model).await;
  let shared = seed_file(&db, &owner.model).await;
  tuple(&db, "file", &shared.to_string(), "viewer", "user", &outsider.model.id.to_string(), "").await;

  for (label, user, expected) in [
    ("the owner", &owner, 3_u64),
    ("an outsider holding one share", &outsider, 1),
  ] {
    let subject = subject_of(&user.model);

    let listed = Authz::filter("file", "view", &subject, Files::find())
      .expect("compiles")
      .all(&db)
      .await
      .expect("runs");
    let counted = Authz::filter("file", "view", &subject, Files::find())
      .expect("compiles")
      .count(&db)
      .await
      .expect("runs");

    assert_eq!(
      listed.len() as u64,
      counted,
      "{label}: the aggregate and the list disagree, which is exactly the G11 shape"
    );
    assert_eq!(counted, expected, "{label}: the scope let through the wrong number of files");
  }

  // The share reaches the file it names and no other. A scope that widened to "any file" would
  // satisfy the equality above while re-opening the disclosure.
  let outsider_subject = subject_of(&outsider.model);
  assert!(
    Authz::check("file", shared, "view", &outsider_subject, &db).await.expect("runs"),
    "the share did not reach its own file"
  );
  assert!(
    !Authz::check("file", mine, "view", &outsider_subject, &db).await.expect("runs"),
    "one share reached a file it does not name"
  );
}

// ─── G12 ─────────────────────────────────────────────────────────────────────────────────────

/// **G12** — `FileService::delete` authorizes against `remove`, not `view`.
///
/// `delete` passed `"read"` to `.action(…)`, so anyone who could see a file could delete it. What
/// made that possible is that the action is a **string**: `view` and `remove` were two values of one
/// parameter, and nothing could tell a wrong one from a right one.
///
/// In the compiled model they are two permissions with two different rules, and this asserts the
/// difference is real rather than nominal — a subject holding only `file#viewer` passes `view` and
/// fails `remove`, on the same file, in the same statement shape.
///
/// The **compile-time** half of G12 — "passing `\"view\"` to a delete path does not compile" — is
/// not assertable yet and is not pretended to be. `try_access` still takes `&str`
/// (`service/mod.rs:39`); it becomes `try_access(perm, id, db)` with `ApiService<E>` in Phase 3
/// (§8.1, §14 step 13), and only then is a wrong permission a type error rather than a wrong value.
#[tokio::test]
async fn g12_view_and_remove_are_different_permissions_not_different_strings() {
  let db = prepare().await;

  let owner = TestUser::create(&db).await.expect("the file's owner");
  let viewer = TestUser::create(&db).await.expect("a caller holding only a view share");

  let uuid = seed_file(&db, &owner.model).await;
  tuple(&db, "file", &uuid.to_string(), "viewer", "user", &viewer.model.id.to_string(), "").await;

  let viewer_subject = subject_of(&viewer.model);
  assert!(
    Authz::check("file", uuid, "view", &viewer_subject, &db).await.expect("runs"),
    "the view share did not confer `view`, so the case below proves nothing"
  );
  assert!(
    !Authz::check("file", uuid, "remove", &viewer_subject, &db).await.expect("runs"),
    "a view share conferred `remove` — G12 is re-opened on the compiled path"
  );

  // The owner holds both. Without this, a rule that refused `remove` to everybody would pass.
  let owner_subject = subject_of(&owner.model);
  assert!(
    Authz::check("file", uuid, "remove", &owner_subject, &db).await.expect("runs"),
    "the owner cannot remove their own file"
  );

  // And the two rules are different *as compiled text*, not merely different in this fixture. A
  // reviewer reading one predicate is the only place the whole decision is visible at once.
  let view_sql = Authz::predicate_sql("file", "view", &viewer_subject).expect("compiles");
  let remove_sql = Authz::predicate_sql("file", "remove", &viewer_subject).expect("compiles");
  assert_ne!(
    view_sql, remove_sql,
    "`file#view` and `file#remove` compile to the same predicate, so the distinction is nominal"
  );
  assert!(
    view_sql.contains("'viewer'") && !remove_sql.contains("'viewer'"),
    "`viewer` must reach `view` and must not reach `remove`\n  view:   {view_sql}\n  remove: {remove_sql}"
  );
}
