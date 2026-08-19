//! The four of design §12.8's twelve fixed criticals that are **sumida's**, re-asserted against the
//! compiled evaluator.
//!
//! Each is also pinned **through HTTP**, in `api/tests/authz_golden.rs`. Those pins were written
//! while the legacy evaluator was in charge and did not have to change when the compiled rules
//! replaced it, because they name a request and a response and never an evaluator.
//!
//! This file asserts the four against the rule directly. Two levels, deliberately: failing here is
//! a rule defect, failing in `authz_golden.rs` is a defect in the code around the rule, and needing
//! both to change at once is the signal that a boundary moved.
//!
//! | # | property | here |
//! |---|---|---|
//! | G9  | a non-admin cannot mint a key in another account's name | `sumida_user#edit` |
//! | G10 | a key bound to an admin does not carry administrative power when secured | `platform:root#superuser` |
//! | G11 | `/files/stats` is scoped the way `list` is | one `file#view` predicate, two statements |
//! | G12 | delete authorizes against `remove`, not `view` | two permissions, not two strings |
//!
//! **The other eight belong to a product and live in that product's repository** — not here. Their
//! row scopes turn on definitions sumida does not declare, whose backing tables are absent from a
//! sumida database entirely, so there is nothing here to evaluate them against. A regression suite
//! for a rule belongs beside the rule.
//!
//! Requires a real Postgres. No request goes through the router: the evaluator is called directly,
//! so a failure here is a rule and not a handler.

use api::authz::Authz;
use api::authz::subject::SubjectSet;
use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{
  ConnectionTrait,
  DatabaseConnection,
  EntityTrait,
  FromQueryResult,
  PaginatorTrait,
  Statement,
};
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
    !Authz::check("sumida_user", victim_id, "edit", &attacker_subject, &db)
      .await
      .expect("runs"),
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
    Authz::check("sumida_user", victim_id, "edit", &attacker_subject, &db)
      .await
      .expect("runs"),
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

  for (label, user, expected) in
    [("the owner", &owner, 3_u64), ("an outsider holding one share", &outsider, 1)]
  {
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
/// still not assertable, and the conversion decided not to make it so. §8.1 proposed a reshaped
/// `ApiService<E>` whose `try_access` would take a typed permission; that trait was rejected rather
/// than rewritten, and then emptied of implementors entirely. Reshaping a trait nobody implements
/// buys nothing. `Authz::try_check` takes the permission as a `&str`, so a wrong one is a wrong
/// *value*. What guards it instead is
/// `golden_sql_tests::every_converted_call_site_names_a_permission_that_exists`: a hand-maintained
/// table of every `(definition, permission)` a converted call site names, which turns a run-time
/// `UnknownPermission` 500 into a unit-test failure. A table somebody has to maintain is weaker
/// than a type, and saying so is the point.
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

// ─── the property the whole design rests on ──────────────────────────────────────────────────

/// For every converted definition: the **list filter and the point check select the same rows**.
///
/// `managed_org_ids` was two implementations of one question — a recursive CTE for the list and a
/// lateral join for the point check — so a caller who passed one and failed the other was a matter
/// of which code path ran. Here both come from the same rewrite tree, and this asserts it over real
/// rows rather than arguing it from the source.
///
/// It also covers the one failure the golden-SQL tests structurally cannot see. Those assert the
/// predicate's *text*; they never see the outer query it drops into, so a definition whose rule
/// reads a table the service's own `default_query_with` has already joined under an alias would
/// pass them and judge the wrong row at run time. Running the predicate inside a real
/// `Select<E>` is what catches that class.
///
/// Each definition is asserted with **one row the caller reaches and one it does not**. A
/// definition where every row answers the same way would satisfy "the two agree" while proving
/// nothing about either.
#[tokio::test]
async fn the_point_check_and_the_list_filter_agree_for_every_converted_definition() {
  let db = prepare().await;

  let tree = OrgTree::create(&db).await.expect("a three-deep organization tree");
  let caller = Member::create(&db, tree.subject).await.expect("the caller");
  let subject = subject_of(&caller.user.model);
  let stranger = TestUser::create(&db).await.expect("somebody with no relationship to the caller");

  // R2: 자기 조직과 그 아래 전부에 닿는 관리자 튜플 하나.
  tuple(
    &db,
    "organization",
    &tree.subject.to_string(),
    "manager",
    "user",
    &caller.user.model.id.to_string(),
    "",
  )
  .await;
  tuple(
    &db,
    "organization",
    &tree.subject.to_string(),
    "user_manager",
    "user",
    &caller.user.model.id.to_string(),
    "",
  )
  .await;

  let inside = Member::create(&db, tree.grandchild).await.expect("a user two levels down");
  let outside = Member::create(&db, tree.aunt).await.expect("a user in another branch");

  let reachable_file = seed_file(&db, &caller.user.model).await;
  let unreachable_file = seed_file(&db, &stranger.model).await;

  let (reachable_group, reachable_board, reachable_post, reachable_attachment) =
    seed_board_world(&db, &caller.user.model).await;
  let (other_group, other_board, other_post, other_attachment) =
    seed_board_world(&db, &stranger.model).await;
  tuple(
    &db,
    "board",
    &reachable_board.to_string(),
    "viewer",
    "user",
    &caller.user.model.id.to_string(),
    "",
  )
  .await;
  tuple(
    &db,
    "board_group",
    &reachable_group.to_string(),
    "viewer",
    "user",
    &caller.user.model.id.to_string(),
    "",
  )
  .await;

  let reachable_topic = seed_topic(&db, reachable_board).await;
  let unreachable_topic = seed_topic(&db, other_board).await;

  let reachable_role = TestRole::create(&db).await.expect("a role the caller may see");
  let unreachable_role = TestRole::create(&db).await.expect("a role the caller may not see");
  tuple(
    &db,
    "role",
    &reachable_role.model.id.to_string(),
    "admin",
    "user",
    &caller.user.model.id.to_string(),
    "",
  )
  .await;

  // 전역 설정 셋은 행에 붙일 관계가 없다 — 서비스 층이 유일한 도달 경로다.
  tuple(&db, "service", "mail_form", "op_read", "user", &caller.user.model.id.to_string(), "").await;
  let reachable_form = seed_mail_form(&db).await;

  let reachable_ban = seed_ban(&db, inside.user.model.id, caller.user.model.id).await;
  let unreachable_ban = seed_ban(&db, stranger.model.id, stranger.model.id).await;

  let reachable_key = seed_api_key(&db, caller.user.model.id).await;
  let unreachable_key = seed_api_key(&db, stranger.model.id).await;

  let reachable_invitation = seed_invitation(&db, inside.user.model.id).await;
  let unreachable_invitation = seed_invitation(&db, stranger.model.id).await;

  let reachable_log = seed_access_log(&db, inside.user.model.id).await;
  let unreachable_log = seed_access_log(&db, stranger.model.id).await;

  // (definition, permission) 마다 "닿아야 하는 행"과 "닿으면 안 되는 행".
  agree::<Users, _>(
    &db,
    "sumida_user",
    "view",
    &subject,
    inside.user.model.id,
    outside.user.model.id,
    |m| m.id,
  )
  .await;
  agree::<Organizations, _>(&db, "organization", "view", &subject, tree.child, tree.aunt, |m| m.id).await;
  agree::<Organizations, _>(&db, "organization", "edit", &subject, tree.child, tree.parent, |m| m.id).await;
  agree::<Roles, _>(
    &db,
    "role",
    "view",
    &subject,
    reachable_role.model.id,
    unreachable_role.model.id,
    |m| m.id,
  )
  .await;
  agree::<Files, _>(&db, "file", "view", &subject, reachable_file, unreachable_file, |m| m.uuid).await;
  agree::<BoardGroups, _>(&db, "board_group", "view", &subject, reachable_group, other_group, |m| m.id)
    .await;
  agree::<Boards, _>(&db, "board", "view", &subject, reachable_board, other_board, |m| m.id).await;
  agree::<BoardTopics, _>(&db, "board_topic", "view", &subject, reachable_topic, unreachable_topic, |m| {
    m.id
  })
  .await;
  agree::<BoardPosts, _>(&db, "board_post", "view", &subject, reachable_post, other_post, |m| m.id).await;
  agree::<BoardFiles, _>(
    &db,
    "board_file",
    "view",
    &subject,
    reachable_attachment,
    other_attachment,
    |m| m.file_uuid,
  )
  .await;
  agree::<Bans, _>(&db, "ban", "view", &subject, reachable_ban, unreachable_ban, |m| m.id).await;
  agree::<ApiKeys, _>(&db, "api_key", "view", &subject, reachable_key.clone(), unreachable_key, |m| {
    m.key.clone()
  })
  .await;
  agree::<UserInvitations, _>(
    &db,
    "user_invitation",
    "view",
    &subject,
    reachable_invitation,
    unreachable_invitation,
    |m| m.uuid,
  )
  .await;
  agree::<AccessLogs, _>(&db, "access_log", "view", &subject, reachable_log, unreachable_log, |m| m.id)
    .await;

  // `mail_form` and `membership_plan` are reachable only through the service tier, so the
  // "unreachable" half is a second caller rather than a second row.
  let outsider = subject_of(&stranger.model);
  let listed: Vec<String> = Authz::filter("mail_form", "view", &subject, MailForms::find())
    .expect("compiles")
    .all(&db)
    .await
    .expect("runs")
    .into_iter()
    .map(|form| form.kind)
    .collect();
  assert!(listed.contains(&reachable_form), "the object-wide grant did not open `mail_form`");
  assert!(
    Authz::check("mail_form", reachable_form.as_str(), "view", &subject, &db)
      .await
      .expect("runs"),
    "`mail_form`'s point check disagrees with its list"
  );
  assert!(
    Authz::filter("mail_form", "view", &outsider, MailForms::find())
      .expect("compiles")
      .all(&db)
      .await
      .expect("runs")
      .is_empty(),
    "a caller with no service-tier grant listed mail forms"
  );
}

/// One definition's list and point check, over one row that must be reachable and one that must not.
async fn agree<E, K>(
  db: &DatabaseConnection,
  definition: &str,
  permission: &str,
  subject: &SubjectSet,
  reachable: K,
  unreachable: K,
  id_of: fn(&E::Model) -> K,
) where
  E: EntityTrait,
  K: Clone + PartialEq + std::fmt::Debug + Into<api::authz::ObjectKey>,
{
  let listed: Vec<K> = Authz::filter(definition, permission, subject, E::find())
    .unwrap_or_else(|e| panic!("`{definition}#{permission}` does not compile to a filter: {e}"))
    .all(db)
    .await
    .unwrap_or_else(|e| panic!("`{definition}#{permission}`'s filter does not run: {e}"))
    .iter()
    .map(id_of)
    .collect();

  for (id, expected) in [(reachable, true), (unreachable, false)] {
    let by_list = listed.contains(&id);
    let by_check = Authz::check(definition, id.clone(), permission, subject, db)
      .await
      .unwrap_or_else(|e| panic!("`{definition}#{permission}`'s point check does not run: {e}"));

    assert_eq!(
      by_list, by_check,
      "`{definition}#{permission}` disagrees with itself about {id:?}: the list said {by_list}, \
       the point check said {by_check} — that is the `managed_org_ids` defect, structurally"
    );
    assert_eq!(
      by_check, expected,
      "`{definition}#{permission}` answered {by_check} for {id:?}; the fixture expects {expected}, \
       so either the grant reaches nothing or it reaches everything and the case is vacuous"
    );
  }
}

// ─── the write half: what replaced `ap::for_users()` ─────────────────────────────────────────

/// `replace_service_grants` replaces the route-gate tier **and nothing else**.
///
/// `ap::for_users().update(..)` deleted by subject alone: submitting the role form wiped every
/// object-tier row that role held as a side effect. Design §9.4 is that a per-object grant survives
/// a submit of the permission grid without the caller having to enumerate what to preserve, and
/// this is where that is asserted — the object-tier tuple is written first and must still be there
/// afterwards.
///
/// The authority half is asserted in the same case because the two are one operation: a caller who
/// may not grant must not get to clear the existing grants on the way to being refused.
#[tokio::test]
async fn replacing_a_roles_service_grants_leaves_its_object_grants_alone() {
  use api::authz::ServiceGrant;
  use api::authz::write::SubjectKey;

  let db = prepare().await;

  let role = TestRole::create(&db).await.expect("a role to grant to");
  let holder = SubjectKey::userset("role", &role.model.id.to_string(), "member");
  let operator = TestUser::create_admin(&db).await.expect("somebody who may grant");
  let ordinary = TestUser::create(&db).await.expect("somebody who may not");

  // 이 역할이 이미 갖고 있는 것들: 라우트 게이트 하나와 개별 객체 부여 하나.
  tuple(&db, "service", "user", "read", "role", &role.model.id.to_string(), "member").await;
  tuple(&db, "board", "7", "editor", "role", &role.model.id.to_string(), "member").await;

  let grants = vec![ServiceGrant {
    namespace: "ban".to_owned(),
    action:    vec!["read".to_owned(), "delete".to_owned()],
  }];

  // (1) 권한 없는 호출자는 거절당하고, **아무것도 지우지 못한다**.
  let refused = Authz::replace_service_grants(&holder, &grants, &subject_of(&ordinary.model), &db).await;
  assert!(refused.is_err(), "a caller with no `service:ban#grant` replaced a role's grants");
  assert_eq!(
    held_by(&db, &holder.key()).await,
    vec!["board:7#editor".to_owned(), "service:user#read".to_owned()],
    "a refused replacement deleted the grants it was refused permission to replace"
  );

  // (2) 권한 있는 호출자가 바꾸면 서비스 층만 바뀐다.
  Authz::replace_service_grants(&holder, &grants, &subject_of(&operator.model), &db)
    .await
    .expect("an administrator may grant");
  assert_eq!(
    held_by(&db, &holder.key()).await,
    vec![
      "board:7#editor".to_owned(),
      "service:ban#delete".to_owned(),
      "service:ban#read".to_owned()
    ],
    "the object-tier grant did not survive a submit of the permission grid (§9.4), or the old \
     route gate was left behind"
  );

  // (3) 화면이 읽어 오는 모양은 그대로다 — 넣은 것과 같은 것이 나온다.
  let read_back = Authz::service_grants(
    &api::authz::subject::SubjectSet::from_parts(vec![holder.key()], Vec::new()),
    &db,
  )
  .await
  .expect("runs");
  assert_eq!(read_back.len(), 1);
  assert_eq!(read_back[0].namespace, "ban");
  let mut actions = read_back[0].action.clone();
  actions.sort();
  assert_eq!(actions, vec!["delete".to_owned(), "read".to_owned()]);

  // (4) `revoke_all_of` 는 서비스 층과 객체 층을 함께 가져간다 — 역할이 사라질 때 쓰인다.
  Authz::revoke_all_of(&holder, &db).await.expect("runs");
  assert!(
    held_by(&db, &holder.key()).await.is_empty(),
    "deleting a role left grants behind in its name"
  );
}

/// `expand_to_user_ids` stops dropping `apikey:` subjects.
///
/// `membership/mod.rs:170` hand-parsed `user:` / `role:` / `org:` and silently ignored everything
/// else, so a key that held the grant never received the notification the grant existed to send.
/// Every subject type the relations accept is expanded here, and the api-key half is asserted
/// explicitly because it is the one that was missing.
#[tokio::test]
async fn expanding_subjects_to_user_ids_includes_api_keys() {
  use api::authz::write::SubjectKey;

  let db = prepare().await;

  let direct = TestUser::create(&db).await.expect("a user named directly");
  let by_role = TestUser::create(&db).await.expect("a user reached through their role");
  let by_org = TestUser::create(&db).await.expect("a user reached through their organization");
  let key_owner = TestUser::create(&db).await.expect("the owner of an api key");
  let unrelated = TestUser::create(&db).await.expect("nobody");

  let key = format!("expand-{}", uuid::Uuid::new_v4().simple());
  ApiKeys::insert(entity::api_keys::ActiveModel {
    key: Set(key.clone()),
    user_id: Set(key_owner.model.id),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("api key");

  let subjects = [
    SubjectKey::user(direct.model.id),
    SubjectKey::userset("role", &by_role.model.role_id.to_string(), "member"),
    SubjectKey::userset("organization", &by_org.model.organization_id.to_string(), "member"),
    SubjectKey::apikey(&key),
  ];

  let ids = Authz::expand_to_user_ids(&subjects, &db).await.expect("runs");

  for (label, expected) in [
    ("a terminal user subject", direct.model.id),
    ("a role userset", by_role.model.id),
    ("an organization userset", by_org.model.id),
    ("an api key", key_owner.model.id),
  ] {
    assert!(ids.contains(&expected), "{label} did not expand to its user: {ids:?}");
  }
  assert!(
    !ids.contains(&unrelated.model.id),
    "the expansion reached somebody no subject named: {ids:?}"
  );

  // 이름 없는 주체는 아무도 내놓지 않는다 — 짐작하지 않는다.
  assert!(
    Authz::expand_to_user_ids(&[SubjectKey::parse("board:7").unwrap()], &db)
      .await
      .expect("runs")
      .is_empty(),
    "a subject type that names no person expanded to somebody"
  );
}

/// Every grant one subject holds, as `type:id#relation`, sorted.
async fn held_by(db: &DatabaseConnection, subject_key: &str) -> Vec<String> {
  #[derive(FromQueryResult)]
  struct Row {
    tuple: String,
  }

  let mut rows = Row::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "SELECT resource_type || ':' || resource_id || '#' || relation AS \"tuple\"
       FROM relation_tuples WHERE subject_key = $1",
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

// ─── seeds for the agreement case ────────────────────────────────────────────────────────────

/// A board group, a board in it, a post by `author`, and a file attached to that post.
async fn seed_board_world(db: &DatabaseConnection, author: &entity::users::Model) -> (i32, i32, i32, Uuid) {
  let suffix = Uuid::new_v4();

  let group_id = BoardGroups::insert(entity::bps::board_groups::ActiveModel {
    name: Set(format!("agree-g-{suffix}")),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("board group")
  .last_insert_id;

  let board_id = Boards::insert(entity::bps::boards::ActiveModel {
    group_id: Set(Some(group_id)),
    name: Set(format!("agree-b-{suffix}")),
    is_enabled: Set(true),
    max_file_size: Set(0),
    max_file_count: Set(0),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("board")
  .last_insert_id;

  let post_id = BoardPosts::insert(entity::bps::board_posts::ActiveModel {
    board_id: Set(board_id),
    user_id: Set(Some(author.id)),
    title: Set(format!("agree-p-{suffix}")),
    content_type: Set(entity::bps::board_posts::BoardPostContentType::Html),
    content: Set(Some("<p>x</p>".to_owned())),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("post")
  .last_insert_id;

  let file_uuid = seed_file(db, author).await;
  BoardFiles::insert(entity::bps::board_files::ActiveModel {
    file_uuid: Set(file_uuid),
    post_id: Set(Some(post_id)),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("attach the file");

  (group_id, board_id, post_id, file_uuid)
}

async fn seed_topic(db: &DatabaseConnection, board_id: i32) -> i32 {
  BoardTopics::insert(entity::bps::board_topics::ActiveModel {
    board_id: Set(board_id),
    name: Set(format!("agree-t-{}", Uuid::new_v4())),
    order: Set(0),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("topic")
  .last_insert_id
}

async fn seed_mail_form(db: &DatabaseConnection) -> String {
  let kind = format!("agree-{}", Uuid::new_v4());
  MailForms::insert(entity::mail_forms::ActiveModel {
    kind: Set(kind.clone()),
    sender: Set("agree@example.com".to_owned()),
    subject: Set("agree".to_owned()),
    content: Set("agree".to_owned()),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("mail form");
  kind
}

/// A **lifted** ban. `check_banned`'s top-level condition is a disjunction over columns whose
/// default is `*`, so one live ban row locks every account out of the login fixture; the row scope
/// under test does not care either way.
async fn seed_ban(db: &DatabaseConnection, target: i32, author: i32) -> i32 {
  Bans::insert(entity::bans::ActiveModel {
    user_id: Set(Some(target)),
    created_by: Set(Some(author)),
    lifted_at: Set(Some(chrono::Utc::now().fixed_offset())),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("ban")
  .last_insert_id
}

async fn seed_api_key(db: &DatabaseConnection, owner: i32) -> String {
  let key = format!("agree-{}", Uuid::new_v4().simple());
  ApiKeys::insert(entity::api_keys::ActiveModel {
    key: Set(key.clone()),
    user_id: Set(owner),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("api key");
  key
}

async fn seed_invitation(db: &DatabaseConnection, invitee: i32) -> Uuid {
  let uuid = Uuid::new_v4();
  UserInvitations::insert(entity::user_invitations::ActiveModel {
    uuid: Set(uuid),
    user_id: Set(invitee),
    expires_at: Set((chrono::Utc::now() + chrono::Duration::days(30)).fixed_offset()),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("invitation");
  uuid
}

async fn seed_access_log(db: &DatabaseConnection, user_id: i32) -> i32 {
  AccessLogs::insert(entity::access_logs::ActiveModel {
    user_id: Set(user_id),
    method: Set(entity::access_logs::AccessMethod::Password),
    code: Set(entity::access_logs::AccessResultCode::Success),
    ip_address: Set("127.0.0.1".to_owned()),
    user_agent: Set("agree".to_owned()),
    os_family: Set("agree".to_owned()),
    device_family: Set("agree".to_owned()),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("access log")
  .last_insert_id
}
