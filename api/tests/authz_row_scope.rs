//! The **object tier**, through the real HTTP stack, one case per converted service.
//!
//! Every case is written against the trap that has produced four false passes in this project: the
//! route gate refuses *before* any row scope runs and answers `403`. A test that forgets to grant
//! the service tier therefore passes with the entire row scope deleted — the request was refused,
//! just not by the thing under test.
//!
//! So each case has the same three parts, and none of them is optional:
//!
//! 1. **grant the route gate first**, so the request reaches the handler at all;
//! 2. assert the gate-only caller is refused by the **row** scope — `Filtered` / `NotFound` /
//!    `Unauthenticated` for a read, and for a write a `403` whose code is `E1008`
//!    (`NoAccessPermission`, the ownership refusal) and **not** `E1009` (`NoServiceAvailable`, the
//!    gate);
//! 3. assert the same caller with the row grant reaches it.
//!
//! Part 3 is what stops "refuse everything" from passing, and part 2 is what stops "grant
//! everything" from passing. A case with only one of them asserts nothing about a scope.
//!
//! Lists are asserted with [`ListProbe::assert_exactly`] rather than "contains mine": a list that
//! correctly returns the caller's own row while also returning every other tenant's satisfies
//! "I got mine".
//!
//! Requires a real Postgres and starts a server per case.

use std::collections::BTreeSet;

use api::util::random::generate_url_safe_string;
use e2e::ap_test_utils::{grant_object, grant_object_wide, grant_org_scope, grant_service};
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::Method;
use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait};
use serde_json::json;
use uuid::Uuid;

// ─── reading a response whose key is not called `id` ─────────────────────────────────────────
//
// `ListProbe::assert_exactly` and `Probe::assert(Visible)` both look for a field literally named
// `id`, which most of this codebase's list items have. Four do not: a file is keyed by `uuid`, a
// mail form by `kind`, an api key by its hash. For those the exactness assertion has to name the
// key — counting the wrong field would silently compare two empty sets and pass.

/// The values of `field` across a list response.
///
/// Panics when the response is not a list at all. That is the load-bearing part: a `403` body has
/// no `items`, and treating it as "zero rows" would let a case that never got past the route gate
/// assert an empty scope and pass.
#[track_caller]
fn ids_of(probe: &ListProbe, field: &str) -> BTreeSet<String> {
  let items = probe.body().get("items").and_then(|items| items.as_array()).unwrap_or_else(|| {
    panic!(
      "the response is not a list — the request was refused before any row scope ran, so this \
       assertion would prove nothing: {}",
      probe.body()
    )
  });

  items
    .iter()
    .filter_map(|item| item.get(field))
    .map(|value| value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()))
    .collect()
}

#[track_caller]
fn assert_listed(probe: &ListProbe, field: &str, expected: &[&str], what: &str) {
  let got = ids_of(probe, field);
  let want: BTreeSet<String> = expected.iter().map(|id| (*id).to_owned()).collect();
  assert_eq!(got, want, "{what} returned the wrong rows (keyed by `{field}`): {}", probe.body());
}

/// Whether a single-resource response actually carries that resource.
fn shows(probe: &Probe, field: &str, value: &str) -> bool {
  probe.body().get(field).and_then(|found| found.as_str()) == Some(value)
}

// ─── fixtures ────────────────────────────────────────────────────────────────────────────────

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

async fn seed_file<C: ConnectionTrait>(db: &C, owner_id: i32) -> Uuid {
  let uuid = Uuid::new_v4();
  Files::insert(entity::fs::files::ActiveModel {
    uuid: Set(uuid),
    user_id: Set(Some(owner_id)),
    original_name: Set(format!("scope-{uuid}.txt")),
    dir: Set("/scope".to_owned()),
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

/// A board group, a board inside it, and a post authored by `author`.
async fn seed_board_post(db: &DatabaseConnection, author_id: i32) -> (i32, i32, i32) {
  let group_id = BoardGroups::insert(entity::bps::board_groups::ActiveModel {
    name: Set(format!("scope-g-{}", rnd())),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("board group")
  .last_insert_id;

  let board_id = Boards::insert(entity::bps::boards::ActiveModel {
    group_id: Set(Some(group_id)),
    name: Set(format!("scope-b-{}", rnd())),
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
    user_id: Set(Some(author_id)),
    title: Set(format!("scope-p-{}", rnd())),
    content_type: Set(entity::bps::board_posts::BoardPostContentType::Html),
    content: Set(Some("<p>x</p>".to_owned())),
    status: Set(entity::bps::board_posts::BoardPostStatus::Published),
    ..Default::default()
  })
  .exec(db)
  .await
  .expect("post")
  .last_insert_id;

  (group_id, board_id, post_id)
}

// ─── users ───────────────────────────────────────────────────────────────────────────────────

/// `sumida_user` — the list and the point check are one predicate, and the organization tier is
/// what opens both.
///
/// This is `managed_org_ids`'s defect made impossible: the list used to be `ap::ns("user")` and the
/// update used to be `ap::ns("org:user")` plus an ancestor walk, so a manager could modify a user
/// they could not enumerate. Both are `sumida_user#…` now.
#[tokio::test]
async fn users_are_scoped_by_the_organization_tier_in_both_directions() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let tree = OrgTree::create(&db).await.expect("tree");
  let manager = Member::create(&db, tree.subject).await.expect("manager");
  let inside = Member::create(&db, tree.grandchild).await.expect("two levels down");
  let outside = Member::create(&db, tree.aunt).await.expect("another branch");

  grant_service(&db, &manager.user, "user", vec!["read".into(), "update".into()])
    .await
    .expect("gate");

  // (2) 게이트만 있는 상태 — 행 스코프가 거절해야 한다. 403 이 나오면 게이트가 막은 것이고,
  // 그러면 이 케이스는 아무것도 증명하지 않는다.
  let session = Session::login(mock.server.addr, &manager.user, "manager of the subject org").await;
  session.assert_authenticated().await;
  session
    .probe_one(&format!("/users/{}", inside.user.model.id), &inside.user.model.id)
    .await
    .assert(Access::Unauthenticated);
  session.probe_list("/users?limit=200").await.assert_empty();

  // (3) 조직 층을 주면 목록과 단건이 함께 열린다 — 그리고 딱 부분트리까지만.
  grant_org_scope(&db, &manager.user, tree.subject, "user_manager")
    .await
    .expect("org tier");

  session
    .probe_one(&format!("/users/{}", inside.user.model.id), &inside.user.model.id)
    .await
    .assert(Access::Visible);
  session
    .probe_one(&format!("/users/{}", outside.user.model.id), &outside.user.model.id)
    .await
    .assert(Access::Unauthenticated);

  let listed = session.probe_list("/users?limit=200").await;
  assert!(
    listed.ids().contains(&inside.user.model.id.to_string()),
    "the list must reach two levels down: {:?}",
    listed.ids()
  );
  assert!(
    !listed.ids().contains(&outside.user.model.id.to_string()),
    "the list left the subtree: {:?}",
    listed.ids()
  );
}

/// The object-wide tier — what `object = '*'` meant (§2.3) — reaches **every** row.
///
/// The acceptance criterion for the `object = '*'` → `service:<ns>#op_read` mapping. Without it the
/// mapping is untested in the direction that matters: a fixture that granted the wrong relation
/// would grant nothing, every "should be forbidden" assertion would still pass, and the capability
/// would be gone with the suite green.
#[tokio::test]
async fn an_object_wide_grant_reaches_rows_the_caller_has_no_share_on() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let operator = TestUser::create(&db).await.expect("operator");
  let stranger_a = TestUser::create(&db).await.expect("a");
  let stranger_b = TestUser::create(&db).await.expect("b");

  grant_service(&db, &operator, "user", vec!["read".into()]).await.expect("gate");

  let session = Session::login(mock.server.addr, &operator, "holder of the gate alone").await;
  session.assert_authenticated().await;
  session.probe_list("/users?limit=200").await.assert_empty();

  grant_object_wide(&db, &operator, "user", vec!["read".into()])
    .await
    .expect("object-wide");

  let listed = session.probe_list("/users?limit=200").await;
  for target in [&stranger_a, &stranger_b] {
    assert!(
      listed.ids().contains(&target.model.id.to_string()),
      "an object-wide read grant did not reach user {} — the `object='*'` mapping grants nothing: \
       {:?}",
      target.model.id,
      listed.ids()
    );
  }

  // …and it stays a read. Object-wide power is per action, exactly as `object='*'` with `{read}` was.
  session
    .probe_write(
      Method::PATCH,
      &format!("/users/{}", stranger_a.model.id),
      Some(&json!({ "fullName": "must not happen" })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1009");
}

// ─── files ───────────────────────────────────────────────────────────────────────────────────

/// `file` — ownership derives from `files.user_id`, a share is one tuple, and the aggregate is the
/// same predicate as the list (G11).
#[tokio::test]
async fn files_are_scoped_by_ownership_and_by_share() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let owner = TestUser::create(&db).await.expect("owner");
  let outsider = TestUser::create(&db).await.expect("outsider");

  let mine = seed_file(&db, owner.model.id).await;
  let shared = seed_file(&db, owner.model.id).await;

  for user in [&owner, &outsider] {
    grant_service(&db, user, "file", vec!["read".into(), "delete".into()])
      .await
      .expect("gate");
  }

  let owner_session = Session::login(mock.server.addr, &owner, "the files' owner").await;
  let outsider_session = Session::login(mock.server.addr, &outsider, "a caller with the gate only").await;
  outsider_session.assert_authenticated().await;

  // 업로더는 정책 행 없이도 자기 파일을 본다 — `file#owner` 가 `files.user_id` 에서 파생되기
  // 때문이다. 종전에는 업로드 때 심어 둔 `ap::edit()` 행이 그 일을 했다.
  assert_listed(
    &owner_session.probe_list("/files?limit=200").await,
    "uuid",
    &[&mine.to_string(), &shared.to_string()],
    "the owner's file list",
  );

  // 게이트만 가진 호출자는 행 스코프에서 걸린다. 403 이면 게이트가 막은 것이다.
  let listed = outsider_session.probe_list("/files?limit=200").await;
  assert_listed(&listed, "uuid", &[], "an outsider's file list");
  assert!(
    !shows(
      &outsider_session.probe_one(&format!("/files/{shared}"), &shared.to_string()).await,
      "uuid",
      &shared.to_string()
    ),
    "an outsider with the gate alone reached a file"
  );

  // 공유 튜플 하나면 그 파일 하나가 열린다 — 다른 파일은 열리지 않는다.
  grant_object(&db, &outsider, "file", &shared.to_string(), vec!["read".into()])
    .await
    .expect("share one file");

  assert_listed(
    &outsider_session.probe_list("/files?limit=200").await,
    "uuid",
    &[&shared.to_string()],
    "the shared-file list",
  );
  assert!(
    !shows(
      &outsider_session.probe_one(&format!("/files/{mine}"), &mine.to_string()).await,
      "uuid",
      &mine.to_string()
    ),
    "one share reached a file it does not name"
  );

  // G11(집계와 목록이 같은 술어라는 것)은 평가기 층에서
  // `authz_regressions.rs::g11_the_file_aggregate_and_the_file_list_are_one_predicate` 가 든다.
  // 여기서 HTTP 로 재확인하지 않는 이유는 `GET /files/stats` 가 이 변경과 무관하게 깨져 있기
  // 때문이다: `SUM(files.size)` 는 Postgres 에서 `numeric` 인데 핸들러가 `Option<i64>` 로 디코딩해
  // 스코프 안에 파일이 하나라도 있으면 500 이 난다. 이 단계의 일이 아니라 그대로 둔다.
}

/// **G12 through HTTP** — a read share does not confer deletion.
///
/// `FileService::delete` used to pass `"read"` to `.action(…)`, so anyone who could see a file could
/// delete it. `view` and `remove` are two different rules now, and this asserts the difference at
/// the endpoint rather than in the rule: the refusal must be `E1008` (the row), because the caller
/// was given `service:file#delete` and therefore passed the gate.
#[tokio::test]
async fn deleting_a_file_needs_remove_and_a_read_share_is_not_it() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let owner = TestUser::create(&db).await.expect("owner");
  let viewer = TestUser::create(&db).await.expect("holder of a read share");
  let uuid = seed_file(&db, owner.model.id).await;

  grant_service(&db, &viewer, "file", vec!["read".into(), "delete".into()])
    .await
    .expect("gate");
  grant_object(&db, &viewer, "file", &uuid.to_string(), vec!["read".into()])
    .await
    .expect("share");

  let session = Session::login(mock.server.addr, &viewer, "read share holder").await;
  assert!(
    shows(
      &session.probe_one(&format!("/files/{uuid}"), &uuid.to_string()).await,
      "uuid",
      &uuid.to_string()
    ),
    "the read share did not confer `view`, so the case below proves nothing"
  );

  // `DELETE /files/{uuid}` 는 `DeleteFile` 본문을 요구한다 — 본문 없이 보내면 415 라서
  // 인가 판정에 닿지도 못한다.
  session
    .probe_write(Method::DELETE, &format!("/files/{uuid}"), Some(&json!({ "hard": false })))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  DbState::new(&db).assert_exists("files", &format!("uuid = '{uuid}'")).await;

  // 소유자는 지울 수 있다. 이게 없으면 "아무도 못 지운다" 로도 위 단언이 통과한다.
  grant_service(&db, &owner, "file", vec!["delete".into()]).await.expect("gate");
  Session::login(mock.server.addr, &owner, "the owner")
    .await
    .probe_write(Method::DELETE, &format!("/files/{uuid}"), Some(&json!({ "hard": false })))
    .await
    .assert(Access::Visible);
  DbState::new(&db).assert_absent("files", &format!("uuid = '{uuid}'")).await;
}

// ─── board posts ─────────────────────────────────────────────────────────────────────────────

/// `board_post` — authorship derives from `board_posts.user_id`, and a stranger who holds the whole
/// route gate is still refused by the row.
///
/// `authorize_post_mutation` used to assemble `(group && board) || owner` from three `effects()`
/// calls; it is one `board_post#edit` now, and the intersection lives in `board#edit`. The stranger
/// case is what proves the row half still runs: they hold `service:board_post#update`, so a refusal
/// can only come from the row.
#[tokio::test]
async fn a_post_is_editable_by_its_author_and_not_by_a_stranger_holding_the_gate() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let author = TestUser::create(&db).await.expect("author");
  let stranger = TestUser::create(&db).await.expect("stranger");
  let (group_id, board_id, post_id) = seed_board_post(&db, author.model.id).await;

  for user in [&author, &stranger] {
    grant_service(&db, user, "board_post", vec!["read".into(), "update".into(), "delete".into()])
      .await
      .expect("gate");
    grant_service(&db, user, "board", vec!["read".into()]).await.expect("board gate");
  }

  let author_session = Session::login(mock.server.addr, &author, "the author").await;
  let stranger_session = Session::login(mock.server.addr, &stranger, "a stranger with the gate").await;
  stranger_session.assert_authenticated().await;

  // 작성자는 정책 행 없이도 자기 글에 닿는다 — `board_post#author` 가 `board_posts.user_id` 에서
  // 파생되기 때문이다. 종전에는 create 가 심어 둔 `ap::edit()` 행이 그 일을 했다.
  author_session
    .probe_one(&format!("/board-posts/{post_id}"), &post_id)
    .await
    .assert(Access::Visible);
  author_session
    .probe_write(Method::PATCH, &format!("/board-posts/{post_id}"), Some(&json!({ "title": "mine" })))
    .await
    .assert(Access::Visible);

  // 게이트를 전부 가진 남은 행에서 막힌다.
  stranger_session
    .probe_write(Method::PATCH, &format!("/board-posts/{post_id}"), Some(&json!({ "title": "theirs" })))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  DbState::new(&db)
    .assert_absent("board_posts", &format!("id = {post_id} AND title = 'theirs'"))
    .await;

  // 게시판 편집 권한을 주면 열린다. 그룹과의 교집합은 `board#edit` 안에 있으므로 호출자가
  // 조립하지 않는다.
  grant_object(&db, &stranger, "board", &board_id.to_string(), vec!["update".into()])
    .await
    .expect("board editor");
  stranger_session
    .probe_write(
      Method::PATCH,
      &format!("/board-posts/{post_id}"),
      Some(&json!({ "title": "moderated" })),
    )
    .await
    .assert(Access::Visible);

  // 그리고 그룹 수준 차단은 작성자의 소유권도 뚫지 못한다 — 빼기가 합집합 바깥에 있다.
  e2e::ap_test_utils::block_object(&db, &author, "board_group", &group_id.to_string())
    .await
    .expect("block at group level");
  author_session
    .probe_write(Method::PATCH, &format!("/board-posts/{post_id}"), Some(&json!({ "title": "blocked" })))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  DbState::new(&db)
    .assert_absent("board_posts", &format!("id = {post_id} AND title = 'blocked'"))
    .await;
}

// ─── the flat resources ──────────────────────────────────────────────────────────────────────

/// `role` — global configuration: no creator, no organization, so only an explicit grant or the
/// service tier reaches a row.
///
/// `role#remove` is the permission this step added. Deleting a role revokes every grant it carries,
/// which is not the same authority as renaming it — so the delete tier is asserted separately from
/// the update tier, on a caller who holds the whole route gate.
#[tokio::test]
async fn a_role_needs_a_row_grant_and_delete_is_its_own_tier() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let target = TestRole::create(&db).await.expect("a role to act on");
  let caller = TestUser::create(&db).await.expect("caller");

  grant_service(&db, &caller, "role", vec!["read".into(), "update".into(), "delete".into()])
    .await
    .expect("gate");

  let session = Session::login(mock.server.addr, &caller, "holder of the role gate").await;
  session.assert_authenticated().await;

  // 게이트만으로는 행에 닿지 못한다.
  session
    .probe_one(&format!("/roles/{}", target.model.id), &target.model.id)
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  session.probe_list("/roles?limit=200").await.assert_empty();

  // 업데이트 층만 주면 읽기·수정은 열리고 삭제는 열리지 않는다.
  grant_object_wide(&db, &caller, "role", vec!["read".into(), "update".into()])
    .await
    .expect("object-wide read/update");

  session
    .probe_one(&format!("/roles/{}", target.model.id), &target.model.id)
    .await
    .assert(Access::Visible);
  session
    .probe_write(
      Method::PATCH,
      &format!("/roles/{}", target.model.id),
      Some(&json!({ "description": "renamed" })),
    )
    .await
    .assert(Access::Visible);
  session
    .probe_write(Method::DELETE, &format!("/roles/{}", target.model.id), None)
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  DbState::new(&db).assert_exists("roles", &format!("id = {}", target.model.id)).await;

  // 삭제 층을 주면 그때 열린다.
  grant_object_wide(&db, &caller, "role", vec!["delete".into()])
    .await
    .expect("object-wide delete");
  session
    .probe_write(Method::DELETE, &format!("/roles/{}", target.model.id), None)
    .await
    .assert(Access::Visible);
  DbState::new(&db).assert_absent("roles", &format!("id = {}", target.model.id)).await;
}

/// `api_key` — a key is reachable by its owner and by whoever manages that owner's organization.
///
/// The list is the interesting half: a caller holding `service:api_key#read` and nothing else used
/// to see every key in the deployment through the `object='*'` catch-all. Now they see their own.
#[tokio::test]
async fn api_keys_are_scoped_to_their_owner() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let owner = TestUser::create(&db).await.expect("owner");
  let other = TestUser::create(&db).await.expect("somebody else");

  for user in [&owner, &other] {
    grant_service(&db, user, "api_key", vec!["read".into(), "create".into()])
      .await
      .expect("gate");
  }

  let owner_session = Session::login(mock.server.addr, &owner, "the key's owner").await;
  let other_session = Session::login(mock.server.addr, &other, "another account with the gate").await;

  owner_session
    .probe_write(Method::POST, "/api-keys", Some(&json!({ "userId": owner.model.id })))
    .await
    .assert(Access::Visible);

  // `ApiKeyListItem` 은 해시가 곧 키라서 `id` 필드가 없다. `ListProbe::assert_exactly` 는
  // `id` 로 세므로 여기서는 항목 수를 직접 본다 — 세는 대상이 다를 뿐 단언의 성격은 같다.
  let count_of = |probe: &ListProbe| -> usize {
    probe.body().get("items").and_then(|items| items.as_array()).map(Vec::len).unwrap_or(0)
  };

  let mine = owner_session.probe_list("/api-keys?limit=200").await;
  assert_eq!(count_of(&mine), 1, "the owner must see exactly their own key: {}", mine.body());

  let theirs = other_session.probe_list("/api-keys?limit=200").await;
  assert_eq!(
    count_of(&theirs),
    0,
    "another account holding only the route gate saw a key it does not own: {}",
    theirs.body()
  );
}

/// `organization` — membership opens `view`, and management opens `edit` and `remove`.
#[tokio::test]
async fn organizations_are_scoped_to_membership_and_management() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let tree = OrgTree::create(&db).await.expect("tree");
  let member = Member::create(&db, tree.subject).await.expect("a plain member");

  grant_service(&db, &member.user, "organization", vec!["read".into(), "update".into()])
    .await
    .expect("gate");

  let session = Session::login(mock.server.addr, &member.user, "a plain member").await;
  session.assert_authenticated().await;

  // 구성원은 자기 조직을 본다(`view = member | manage | …`) — 부여 없이도.
  session
    .probe_one(&format!("/organizations/{}", tree.subject), &tree.subject)
    .await
    .assert(Access::Visible);
  // 다른 가지는 보이지 않는다.
  // 스코프가 걸러 낸 조직은 404 다(`controller/organization.rs` 가 `ok_or(NoData)` 로 답한다).
  session
    .probe_one(&format!("/organizations/{}", tree.aunt), &tree.aunt)
    .await
    .assert(Access::NotFound);
  // 그러나 구성원일 뿐이므로 고칠 수는 없다 — 게이트는 통과했으니 거절은 행에서 나온다.
  session
    .probe_write(
      Method::PATCH,
      &format!("/organizations/{}", tree.subject),
      Some(&json!({ "name": format!("renamed-{}", rnd()) })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");

  // 관리자 튜플 하나면 자기 조직과 그 아래 전부에 닿는다.
  grant_org_scope(&db, &member.user, tree.subject, "manager").await.expect("manager");
  session
    .probe_write(
      Method::PATCH,
      &format!("/organizations/{}", tree.child),
      Some(&json!({ "name": format!("renamed-{}", rnd()) })),
    )
    .await
    .assert(Access::Visible);
  // 위쪽으로는 여전히 닿지 않는다. R2 는 아래로만 간다.
  session
    .probe_write(
      Method::PATCH,
      &format!("/organizations/{}", tree.parent),
      Some(&json!({ "name": format!("renamed-{}", rnd()) })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
}

/// `ban` — the author of a ban sees it, and so does whoever can see the banned user.
#[tokio::test]
async fn bans_are_scoped_to_their_author_and_to_the_banned_users_managers() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let tree = OrgTree::create(&db).await.expect("tree");
  let author = Member::create(&db, tree.subject).await.expect("the ban's author");
  let target = Member::create(&db, tree.child).await.expect("the banned user");
  let bystander = TestUser::create(&db).await.expect("an unrelated account");

  // `lifted_at` 을 채워 둔다. `check_banned` 의 조건은 최상위가 `Condition::any()` 라서
  // ip/user_agent/os/device 열이 전부 기본값 `*` 인 행 하나면 **모든** 계정의 로그인이 막힌다.
  // 여기서 보려는 것은 `bans` 행의 열람 스코프이지 차단 동작이 아니므로, 해제된 처분을 쓴다.
  let ban_id = Bans::insert(entity::bans::ActiveModel {
    user_id: Set(Some(target.user.model.id)),
    created_by: Set(Some(author.user.model.id)),
    reason: Set(Some("scope".to_owned())),
    lifted_at: Set(Some(chrono::Utc::now().fixed_offset())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("seed a ban")
  .last_insert_id;

  for user in [&author.user, &bystander] {
    grant_service(&db, user, "ban", vec!["read".into()]).await.expect("gate");
  }

  Session::login(mock.server.addr, &author.user, "the ban's author")
    .await
    .probe_list("/bans?limit=200")
    .await
    .assert_exactly(&[&ban_id]);

  let outsider = Session::login(mock.server.addr, &bystander, "an unrelated holder of the gate").await;
  outsider.assert_authenticated().await;
  outsider.probe_list("/bans?limit=200").await.assert_empty();
}

/// `access_log`, `user_invitation`, `mail_form` and `membership_plan` — the four whose row tier is
/// the service tier and nothing else.
///
/// They have no creator and no organization, so `object = '*'` was the only way to reach them and
/// `service:<ns>#op_read` is its exact replacement. Grouped into one case because the assertion is
/// identical and the mechanism is one: the gate is not enough, the object-wide grant is.
#[tokio::test]
async fn the_global_resources_open_only_at_the_object_wide_tier() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  MailForms::insert(entity::mail_forms::ActiveModel {
    kind: Set(format!("scope-{}", rnd())),
    sender: Set("scope@example.com".to_owned()),
    subject: Set("scope".to_owned()),
    content: Set("scope".to_owned()),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("seed a mail form");

  let caller = TestUser::create(&db).await.expect("caller");
  for namespace in ["access_log", "user_invitation", "mail_form", "membership_plan"] {
    grant_service(&db, &caller, namespace, vec!["read".into()]).await.expect("gate");
  }

  let session = Session::login(mock.server.addr, &caller, "holder of four gates and no rows").await;
  session.assert_authenticated().await;

  // 목록 키가 엔드포인트마다 다르다. `assert_empty()` 는 `id` 만 세므로 `kind` 로 식별되는
  // 메일 폼에는 아무것도 검사하지 않는다 — 그래서 각 엔드포인트의 실제 키로 센다.
  for (path, key) in [("/access-logs", "id"), ("/mail-forms", "kind"), ("/membership-plans", "id")] {
    let listed = session.probe_list(&format!("{path}?limit=200")).await;
    assert_listed(&listed, key, &[], &format!("{path} for a caller holding the gate alone"));
  }

  for namespace in ["access_log", "mail_form", "membership_plan"] {
    grant_object_wide(&db, &caller, namespace, vec!["read".into()])
      .await
      .expect("object-wide");
  }

  // 메일 폼은 시드해 두었으므로 열렸는지 셀 수 있다. 나머지 둘은 목록이 비어 있을 수 있어
  // 200 을 확인하는 것으로 충분하다 — 비어 있는 것과 거절된 것을 harness 가 구분한다.
  let forms = session.probe_list("/mail-forms?limit=200").await;
  assert!(
    !ids_of(&forms, "kind").is_empty(),
    "an object-wide read grant did not open `mail_form`: {}",
    forms.body()
  );
}

/// `board_file` — an attachment's delete is authorized through its post, never by holding the
/// attachment namespace alone.
///
/// This is §12.8's containment shape: a child row reached by id must not skip the scope its parent
/// applies. The old code joined `board_files.post_id` and compared it against the policy's object,
/// so the *post* id had to be granted; the definition is keyed by `board_files.file_uuid` and
/// resolves through `post->edit`, so the grant that opens it is a grant on the board.
///
/// The **delete** path is used rather than the read path because `GET /board-files` and
/// `GET /board-files/file` are broken independently of this change — the list projects the key as
/// `id` while `BoardFileListItem` reads `file_uuid`, and the `get` route declares two path
/// parameters it does not bind. Both predate the routing and neither is fixed here.
#[tokio::test]
async fn deleting_a_board_file_is_authorized_through_its_post() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let author = TestUser::create(&db).await.expect("author");
  let outsider = TestUser::create(&db).await.expect("outsider");
  let (_group_id, _board_id, post_id) = seed_board_post(&db, author.model.id).await;

  let file_uuid = seed_file(&db, author.model.id).await;
  BoardFiles::insert(entity::bps::board_files::ActiveModel {
    file_uuid: Set(file_uuid),
    post_id: Set(Some(post_id)),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("attach the file");

  grant_service(&db, &outsider, "board_file", vec!["delete".into()]).await.expect("gate");
  grant_service(&db, &outsider, "file", vec!["delete".into()]).await.expect("file gate");

  let session = Session::login(mock.server.addr, &outsider, "a holder of the board_file gate").await;
  session.assert_authenticated().await;

  let body = json!({ "postId": post_id, "fileUuid": file_uuid, "hard": false });

  // 게이트를 전부 가졌는데도 행에서 막힌다. E1008 이어야 하고, E1009 면 게이트가 막은 것이라
  // 이 케이스는 아무것도 증명하지 않는다.
  session
    .probe_write(Method::DELETE, "/board-files", Some(&body))
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  DbState::new(&db)
    .assert_exists("board_files", &format!("file_uuid = '{file_uuid}'"))
    .await;

  // 긍정 절반은 글 작성자가 수행한다. `board_file#remove = post->edit | file->remove` 이고,
  // `BoardFileService::delete` 는 원본 파일까지 지우므로 `file#remove` 도 필요하다 — 그 둘을
  // 함께 가진 사람은 작성자뿐이다. 이게 없으면 "아무도 못 지운다" 로도 위 단언이 통과한다.
  grant_service(&db, &author, "board_file", vec!["delete".into()]).await.expect("gate");
  grant_service(&db, &author, "file", vec!["delete".into()]).await.expect("file gate");
  Session::login(mock.server.addr, &author, "the post's author")
    .await
    .probe_write(Method::DELETE, "/board-files", Some(&body))
    .await
    .assert(Access::Visible);
  DbState::new(&db)
    .assert_absent("board_files", &format!("file_uuid = '{file_uuid}'"))
    .await;
}

/// `board_topic` and `board_group` — the two that resolve entirely through another definition.
#[tokio::test]
async fn board_topics_follow_their_board_and_groups_carry_their_children() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let author = TestUser::create(&db).await.expect("author");
  let caller = TestUser::create(&db).await.expect("caller");
  let (group_id, board_id, _post_id) = seed_board_post(&db, author.model.id).await;

  let topic_id = BoardTopics::insert(entity::bps::board_topics::ActiveModel {
    board_id: Set(board_id),
    name: Set(format!("scope-t-{}", rnd())),
    order: Set(0),
    ..Default::default()
  })
  .exec(&db)
  .await
  .expect("seed a topic")
  .last_insert_id;

  for namespace in ["board", "board_group", "board_topic"] {
    grant_service(&db, &caller, namespace, vec!["read".into()]).await.expect("gate");
  }

  let session = Session::login(mock.server.addr, &caller, "holder of three gates and no rows").await;
  session.assert_authenticated().await;
  session.probe_list("/board-topics?limit=200").await.assert_empty();
  session.probe_list("/boards?limit=200").await.assert_empty();

  // 그룹만 열어도 게시판은 열리지 않는다 — `board#view` 의 그룹 갈래는 교집합이기 때문이다
  // (`group->view & viewer_or_edit_here`). 종전 `ApChain` 이 호출자 쪽에서 조립하던 AND 가
  // 규칙 안으로 옮겨간 것이고, 합집합으로 완화되지 않았다는 것이 여기서 확인된다.
  grant_object(&db, &caller, "board_group", &group_id.to_string(), vec!["read".into()])
    .await
    .expect("group viewer");
  session.probe_list("/boards?limit=200").await.assert_empty();
  session.probe_list("/board-topics?limit=200").await.assert_empty();

  // 게시판 자체를 열면 그 아래 토픽이 따라 열린다(`board_topic#view = board->view`).
  grant_object(&db, &caller, "board", &board_id.to_string(), vec!["read".into()])
    .await
    .expect("board viewer");
  session.probe_list("/boards?limit=200").await.assert_exactly(&[&board_id]);
  session.probe_list("/board-topics?limit=200").await.assert_exactly(&[&topic_id]);
}

// ─── the write half, end to end ──────────────────────────────────────────────────────────────

/// The role form's wire contract survives, and the caller's own payload changes with it.
///
/// `RoleForm.vue` posts `accessPolicies: [{namespace, object:["service"], action, effect}]` and
/// reads the same shape back. That contract is deliberately unchanged (§9.4) — what changed is
/// where the grants live, so the round trip is the assertion: post, read back, and then check that
/// a member of that role actually receives the capability through `/me/access-policies`.
///
/// The last part is what makes it more than a serialisation test. A round trip that stored the
/// grant somewhere nobody reads would pass every step but the last.
#[tokio::test]
async fn the_role_form_round_trips_and_the_grant_reaches_the_roles_members() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let admin = TestUser::create_admin(&db).await.expect("an administrator");
  let session = Session::login(mock.server.addr, &admin, "an administrator").await;

  let created = session
    .probe_write(
      Method::POST,
      "/roles",
      Some(&json!({
        "name": format!("form-{}", rnd()),
        "description": "the permission grid",
        "accessPolicies": [
          { "namespace": "ban", "object": ["service"], "action": ["read", "delete"], "effect": "allow" }
        ]
      })),
    )
    .await
    .assert(Access::Visible);
  let role_id = created.body()["id"].as_i64().expect("the form must create a role") as i32;

  // 읽어 오는 모양이 넣은 것과 같다.
  let read = session.probe_one(&format!("/roles/{role_id}"), &role_id).await.assert(Access::Visible);
  let policies = read.body()["accessPolicies"].as_array().cloned().unwrap_or_default();
  let entry = policies
    .iter()
    .find(|policy| policy["namespace"] == "ban")
    .unwrap_or_else(|| panic!("the grant did not come back: {}", read.body()));
  assert_eq!(entry["object"], json!(["service"]), "the client concatenates this literal");
  assert_eq!(entry["effect"], "allow");
  let mut actions = entry["action"]
    .as_array()
    .unwrap()
    .iter()
    .map(|a| a.as_str().unwrap())
    .collect::<Vec<_>>();
  actions.sort_unstable();
  assert_eq!(actions, vec!["delete", "read"]);

  // 그리고 그 역할의 구성원이 실제로 그 능력을 받는다.
  let member = TestUser::create_in_role(&db, role_id).await.expect("a member of the role");
  let member_session = Session::login(mock.server.addr, &member, "a member of the role").await;
  let payload = member_session.probe_one("/me/access-policies", &"policies").await;
  let received = payload.body().as_array().cloned().unwrap_or_default();
  assert!(
    received.iter().any(|policy| policy["namespace"] == "ban"
      && policy["object"] == json!(["service"])
      && policy["action"].as_array().is_some_and(|a| a.iter().any(|x| x == "read"))),
    "a member of the role did not receive the grant the form stored: {}",
    payload.body()
  );
  member_session.probe_list("/bans?limit=200").await.assert_empty();

  // 객체 층은 더 이상 이 응답에 실리지 않는다 — 서버가 그 표를 보지 않기 때문이다.
  assert!(
    received
      .iter()
      .all(|policy| policy["object"] != json!(["*"]) || policy["namespace"] == "*"),
    "an object-tier entry reached the payload: {}",
    payload.body()
  );

  // 역할을 지우면 그 역할 앞으로 남은 부여도 사라진다. 구성원이 있는 역할은 `users.role_id` 가
  // NOT NULL 이라 애초에 지워지지 않으므로, 이 절반은 구성원 없는 두 번째 역할로 확인한다.
  let disposable = session
    .probe_write(
      Method::POST,
      "/roles",
      Some(&json!({
        "name": format!("form-{}", rnd()),
        "description": "created to be deleted",
        "accessPolicies": [
          { "namespace": "ban", "object": ["service"], "action": ["read"], "effect": "allow" }
        ]
      })),
    )
    .await
    .assert(Access::Visible);
  let disposable_id = disposable.body()["id"].as_i64().expect("a role") as i32;
  DbState::new(&db)
    .assert_exists("relation_tuples", &format!("subject_key = 'role:{disposable_id}#member'"))
    .await;

  session
    .probe_write(Method::DELETE, &format!("/roles/{disposable_id}"), None)
    .await
    .assert(Access::Visible);
  DbState::new(&db)
    .assert_absent("relation_tuples", &format!("subject_key = 'role:{disposable_id}#member'"))
    .await;
}

/// A block is refused by the form rather than stored as a grant.
///
/// `effect: "deny"` had a meaning the tuple store cannot hold — there is no negative tuple and no
/// `effect` column (§3). Accepting it and writing an `allow` would report a block that does not
/// exist, which is the worst of the three possible answers.
#[tokio::test]
async fn the_role_form_refuses_a_deny_instead_of_storing_it_as_a_grant() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let admin = TestUser::create_admin(&db).await.expect("an administrator");
  let session = Session::login(mock.server.addr, &admin, "an administrator").await;

  let name = format!("deny-{}", rnd());
  let probe = session
    .probe_write(
      Method::POST,
      "/roles",
      Some(&json!({
        "name": name,
        "description": "a block the store cannot hold",
        "accessPolicies": [
          { "namespace": "ban", "object": ["service"], "action": ["read"], "effect": "deny" }
        ]
      })),
    )
    .await;

  assert_eq!(
    probe.access(),
    Access::Other(400),
    "a `deny` must be refused as a malformed request, not stored: {}",
    probe.body()
  );
  DbState::new(&db).assert_absent("roles", &format!("name = '{name}'")).await;
}
