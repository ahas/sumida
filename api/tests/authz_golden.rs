//! Golden tests: the **outcome** of an authorization decision, through HTTP.
//!
//! Written before the ReBAC rewrite so it could be proved equivalent rather than argued to be. The
//! acceptance bar was not "identical mechanism" but "the new model can be configured to produce the
//! same outcome", so what is written down is the outcome — for each tier of caller, against each
//! shape of resource, which rows come back and which layer refuses.
//!
//! Every case goes through the router and asserts on a response, so it never named an evaluator and
//! did not have to change when one replaced the other. That is why it survived §14 step 14, which
//! deleted `util/ap.rs` and with it `authz_equivalence.rs`: these cases were the "before" of the
//! comparison, they are the "after" unchanged, and a case that is both is the strongest evidence
//! the migration produces.
//!
//! **A golden test is not an endorsement.** Some of what is pinned here is behaviour this codebase
//! changed on purpose, and locking a defect in place under the name "golden" would be worse than
//! having no test at all. So every case is labelled:
//!
//! - `pin_*` — behaviour that had to survive the rewrite. A change here is a regression.
//! - `today_*` — behaviour recorded because it was what happened before, and which the rewrite was
//!   expected to change. These document the starting point; when one of them fails, read the
//!   comment and decide whether the new answer is the intended one.
//!
//! Requires a real Postgres — every scope under test lives in SQL.

use api::util::random::generate_url_safe_string;
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::Method;
use sea_orm::ActiveValue::Set;
use sea_orm::EntityTrait;
use serde_json::json;

// ─── regressions: the three sumida fixes from 67857ef ────────────────────────

/// An API key may not be issued in someone else's name.
///
/// `CreateApiKey.user_id` was written verbatim from the request body while `verify_api_key`
/// authorizes the session as the key's owner, and `is_secured` defaults to false so that session
/// inherits the owner's full permissions — `is_admin` included. Holding the coarse
/// `api_key:create` capability was therefore enough to mint an administrator's key and use it.
///
/// Enforced by a **handler check**, so the expected refusal is `Forbidden`, not a filtered row.
#[tokio::test]
async fn pin_api_key_cannot_be_issued_for_another_user() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let attacker = TestUser::create(&db).await.expect("attacker");
  let victim = TestUser::create_admin(&db).await.expect("victim admin");
  let session = Session::login(mock.server.addr, &attacker, "non-admin holding api_key:create").await;

  session
    .probe_write(Method::POST, "/api-keys", Some(&json!({ "userId": victim.model.id })))
    .await;

  // 이 단언이 본체다. 응답 코드가 무엇이든 남의 명의 키가 만들어지지 않아야 한다.
  DbState::new(&db)
    .assert_absent("api_keys", &format!("user_id = {}", victim.model.id))
    .await;
}

/// A caller may issue a key for themselves. Pinned alongside the case above so a fix that simply
/// refuses everything cannot pass — that would break the feature while looking secure.
#[tokio::test]
async fn pin_api_key_for_self_is_still_possible_or_gated_only_by_capability() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create_admin(&db).await.expect("admin");
  let session = Session::login(mock.server.addr, &user, "admin issuing for self").await;

  let probe = session
    .probe_write(Method::POST, "/api-keys", Some(&json!({ "userId": user.model.id })))
    .await;

  assert!(
    !matches!(probe.access(), Access::Forbidden),
    "issuing a key for oneself must not be refused — got {:?}",
    probe.access()
  );
  DbState::new(&db).assert_exists("api_keys", &format!("user_id = {}", user.model.id)).await;
}

/// `GET /files/stats` aggregated over every tenant's files while its sibling `list` went through
/// the access-policy-scoped select. An aggregate is a disclosure: it answers "how much does that
/// tenant have" without returning a row.
///
/// A caller with no file grants must get a zeroed aggregate, **not** a refusal — the endpoint is
/// scoped, not gated, and pinning the wrong one would let a future change swap them silently.
#[tokio::test]
async fn pin_file_stats_are_scoped_not_global() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let outsider = TestUser::create(&db).await.expect("outsider");

  // 라우트 게이트(`service_ap!`)를 먼저 통과시킨다. 이걸 주지 않으면 403 에서 끝나고
  // 행 스코프는 실행조차 되지 않아, 스코프를 전부 지워도 이 테스트는 통과한다.
  e2e::ap_test_utils::grant_service(&db, &outsider, "file", vec!["read".into()])
    .await
    .expect("grant the file route capability");

  let session = Session::login(mock.server.addr, &outsider, "user with the file capability but no file grants").await;
  let probe = session.probe_one("/files/stats", &"stats").await;

  assert_ne!(
    probe.access(),
    Access::Forbidden,
    "the route gate refused, so the scope under test never ran — this case proves nothing"
  );

  // 다른 테넌트의 파일 수가 여기 새어 나오면 안 된다.
  let total = probe.body().get("totalFiles").and_then(|v| v.as_i64());

  assert_eq!(
    total,
    Some(0),
    "file stats did not return a scoped zero for a caller with no file grants: {}",
    probe.body()
  );
}

// ─── the tier boundary as it stands ──────────────────────────────────────────

/// An administrator sees every user. This is R1 and it is the one tier that works today.
#[tokio::test]
async fn pin_admin_sees_every_user() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let admin = TestUser::create_admin(&db).await.expect("admin");
  let a = TestUser::create(&db).await.expect("a");
  let b = TestUser::create(&db).await.expect("b");

  let session = Session::login(mock.server.addr, &admin, "admin").await;
  let listed = session.probe_list("/users?limit=200").await;

  for u in [&a, &b, &admin] {
    assert!(
      listed.ids().contains(&u.model.id.to_string()),
      "admin could not see user {} — R1 is broken",
      u.model.id
    );
  }
}

/// **Today's behaviour, expected to change.**
///
/// `UserService::list` resolves through `ap::ns("user")`, whose object is a *user id*. No
/// row-level policy exists in any deployment, so an ordinary caller sees nothing at all. That is
/// fail-safe rather than fail-open, which is why it is not a critical — but it is also why the
/// organization tier cannot be expressed today: the only way to widen this list is a policy per
/// user id, or `object = "*"`, which opens every tenant at once.
///
/// After the rewrite an organization manager should see their own organization and its
/// descendants here. When that happens this test fails, and the new answer is the intended one.
#[tokio::test]
async fn today_plain_member_sees_no_users_at_all() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let tree = OrgTree::create(&db).await.expect("tree");
  let member = Member::create(&db, tree.subject).await.expect("member");
  let _peer = Member::create(&db, tree.subject).await.expect("peer in the same org");
  let _below = Member::create(&db, tree.child).await.expect("member of a descendant org");

  // 게이트를 통과시켜야 행 스코프가 실제로 평가된다.
  e2e::ap_test_utils::grant_service(&db, &member.user, "user", vec!["read".into()])
    .await
    .expect("grant the user route capability");

  let session = Session::login(mock.server.addr, &member.user, "member with the user capability").await;
  let listed = session.probe_list("/users?limit=200").await;

  listed
    .assert_refused(Access::Visible)
    .assert_empty();
}

/// **This was `today_update_and_read_disagree_about_the_org_tier`, and the disagreement is gone.**
///
/// The asymmetry at the heart of the audit: within one service, `update` was hierarchy-aware
/// (`ap::ns("org:user")` composed with the ancestor walk) while `list`, `get` and `delete` resolved
/// through `ap::ns("user")` and were not. A manager granted authority over an organization could
/// therefore modify a user they could not see or enumerate.
///
/// Both sides are `sumida_user#edit` / `sumida_user#view` now, generated from one rewrite tree, so
/// the two cannot diverge by construction. What is asserted is the agreement in **both**
/// directions: the manager reaches the descendant org's user through read, list and update; a
/// caller holding the same route gate and no organization tier reaches none of them. Asserting only
/// the first would pass on a scope that had been deleted outright.
#[tokio::test]
async fn pin_update_and_read_agree_about_the_org_tier() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let tree = OrgTree::create(&db).await.expect("tree");
  let manager = Member::create(&db, tree.subject).await.expect("manager");
  let target = Member::create(&db, tree.child).await.expect("target in a descendant org");
  let outside = Member::create(&db, tree.aunt).await.expect("a user in another branch");

  // 읽기와 쓰기 양쪽 게이트를 준다. 그래야 두 경로의 *행 스코프* 가 실제로 평가된다.
  e2e::ap_test_utils::grant_service(
    &db,
    &manager.user,
    "user",
    vec!["read".into(), "update".into()],
  )
  .await
  .expect("grant the user route capability");
  // 조직 계층 권한. 종전 `org:user | <subject org> | {update}` 가 뜻하던 층이다.
  e2e::ap_test_utils::grant_org_scope(&db, &manager.user, tree.subject, "user_manager")
    .await
    .expect("grant org-tier update over the subject org");

  let session = Session::login(mock.server.addr, &manager.user, "manager of subject org").await;
  session.assert_authenticated().await;

  // 단건 읽기 — 종전에는 행 스코프가 걸러 내어 401 이 나오던 자리다.
  session
    .probe_one(&format!("/users/{}", target.user.model.id), &target.user.model.id)
    .await
    .assert(Access::Visible);

  // 쓰기 — 종전에도 통과했다. 둘이 이제 같은 규칙에서 나온다는 것이 요점이다.
  session
    .probe_write(
      Method::PATCH,
      &format!("/users/{}", target.user.model.id),
      Some(&json!({ "fullName": "reached by the org tier" })),
    )
    .await
    .assert(Access::Visible);

  // 목록도 같은 술어다. 다른 가지의 사용자는 여기 새어 나오면 안 된다.
  let listed = session.probe_list("/users?limit=200").await;
  assert!(
    listed.ids().contains(&target.user.model.id.to_string()),
    "the list does not reach the descendant org the point check reaches — the two have diverged \
     again: {:?}",
    listed.ids()
  );
  assert!(
    !listed.ids().contains(&outside.user.model.id.to_string()),
    "the list left the manager's subtree: {:?}",
    listed.ids()
  );

  // 반대 방향. 같은 라우트 게이트만 가진 호출자는 어느 쪽으로도 닿지 못한다 — 이게 없으면
  // 행 스코프를 통째로 지워도 위 단언들은 그대로 통과한다.
  let bystander = Member::create(&db, tree.subject).await.expect("a peer with no organization tier");
  e2e::ap_test_utils::grant_service(
    &db,
    &bystander.user,
    "user",
    vec!["read".into(), "update".into()],
  )
  .await
  .expect("grant the user route capability");

  let peer = Session::login(mock.server.addr, &bystander.user, "peer with the gate only").await;
  peer.assert_authenticated().await;
  peer
    .probe_one(&format!("/users/{}", target.user.model.id), &target.user.model.id)
    .await
    .assert(Access::Unauthenticated);
  peer.probe_list("/users?limit=200").await.assert_empty();
  peer
    .probe_write(
      Method::PATCH,
      &format!("/users/{}", target.user.model.id),
      Some(&json!({ "fullName": "must not happen" })),
    )
    .await
    .assert(Access::Forbidden)
    .assert_code("E1008");
  DbState::new(&db)
    .assert_absent("users", &format!("id = {} AND full_name = 'must not happen'", target.user.model.id))
    .await;
}

// ─── containment: a child row must inherit its parent's scope ────────────────

/// A resource reached directly by id must not skip the scope its parent applies.
///
/// Three of the audit's criticals were exactly this: `GET /tasks/{id}/logs` and friends filtered
/// by the parent's id and never asked whether the caller may see the parent. The equivalent in
/// sumida is a board file, whose access is resolved through its post and then its board.
///
/// Pinned as "not visible" rather than a specific status because which layer refuses is itself
/// under review; what must not change is that an unrelated caller cannot read it.
#[tokio::test]
async fn pin_child_row_by_id_does_not_bypass_its_parent() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let outsider = TestUser::create(&db).await.expect("outsider");
  e2e::ap_test_utils::grant_service(&db, &outsider, "file", vec!["read".into()])
    .await
    .expect("grant the file route capability");
  let session = Session::login(mock.server.addr, &outsider, "unrelated caller with the capability").await;

  // 게이트를 통과한 뒤에도 행 스코프가 막아야 한다. 존재 여부가 응답으로 새면
  // 그 자체가 열거 창구가 된다.
  let probe = session.probe_one("/files/00000000-0000-0000-0000-000000000001", &"x").await;

  assert_ne!(
    probe.access(),
    Access::Forbidden,
    "the route gate refused, so the row scope never ran — this case proves nothing"
  );

  assert_ne!(
    probe.access(),
    Access::Visible,
    "an unrelated caller reached a file by id: {}",
    probe.body()
  );
}

// ─── the block that ownership must not circumvent ────────────────────────────

/// The property `board_post::authorize_post_mutation` is built around: a block applies even to the
/// author of the post.
///
/// ```text
/// allowed = (group_allow && board_allow) || owner_allow
/// denied  = group_deny  || board_deny   || owner_deny
/// result  = allowed && !denied
/// ```
///
/// The rewrite replaces per-tuple `deny` with an `exclusion` in the rule, which expresses the same
/// thing — but only if the rule actually subtracts the block. This is what makes that concrete: if
/// the exclusion is forgotten, an author walks through their own block and this goes red.
///
/// Written against a **real board and a real post the caller authored**, so the ownership branch
/// is genuinely taken. An earlier version of this case pointed at a post id that did not exist and
/// "passed" on a 404 — proving nothing at all.
#[tokio::test]
async fn pin_a_block_is_not_circumvented_by_authorship() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let author = TestUser::create(&db).await.expect("author");

  // 게시판 그룹 → 게시판. 두 단계가 있어야 group/board/owner 세 갈래가 모두 평가된다.
  let group_id = BoardGroups::insert(entity::bps::board_groups::ActiveModel {
    name: Set(format!("g-{}", generate_url_safe_string(6).unwrap())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .unwrap()
  .last_insert_id;

  let board_id = Boards::insert(entity::bps::boards::ActiveModel {
    group_id: Set(Some(group_id)),
    name: Set(format!("b-{}", generate_url_safe_string(6).unwrap())),
    is_enabled: Set(true),
    max_file_size: Set(0),
    max_file_count: Set(0),
    ..Default::default()
  })
  .exec(&db)
  .await
  .unwrap()
  .last_insert_id;

  // 작성자에게 글을 쓸 권한을 전부 준다.
  let actions = vec!["create".to_string(), "read".to_string(), "update".to_string()];
  for namespace in ["board_post", "board"] {
    e2e::ap_test_utils::grant_service(&db, &author, namespace, actions.clone())
      .await
      .expect("grant the route gate");
  }
  e2e::ap_test_utils::grant_object(&db, &author, "board", &board_id.to_string(), actions.clone())
    .await
    .expect("grant the board");
  e2e::ap_test_utils::grant_object(&db, &author, "board_group", &group_id.to_string(), actions)
    .await
    .expect("grant the group");

  let session = Session::login(mock.server.addr, &author, "author").await;
  session.assert_authenticated().await;

  let created = session
    .probe_write(
      Method::POST,
      "/board-posts",
      Some(&json!({
        "boardId": board_id, "title": "mine", "content": "<p>x</p>",
        "contentType": "text/html", "kind": "article", "status": "published",
        "contentVisibility": "public"
      })),
    )
    .await;
  let post_id = created.body()["id"].as_i64().expect("the fixture must create a real post") as i32;

  // 차단 전: 작성자는 자기 글을 고칠 수 있다. 이 확인이 없으면 아래 단언이
  // "원래 못 고쳤다" 로도 통과해 버린다.
  session
    .probe_write(Method::PATCH, &format!("/board-posts/{post_id}"), Some(&json!({ "title": "edited" })))
    .await
    .assert(Access::Visible);

  // 이제 게시판 수준에서 차단한다. 소유권은 그대로다. 종전에는 `effect = 'deny'` 한 행이었고,
  // 지금은 `board:<id>#blocked` 튜플이다 — 규칙에서 합집합 바깥으로 빠진다.
  e2e::ap_test_utils::block_object(&db, &author, "board", &board_id.to_string())
    .await
    .expect("block at board level");

  let after = session
    .probe_write(Method::PATCH, &format!("/board-posts/{post_id}"), Some(&json!({ "title": "blocked" })))
    .await;

  assert_ne!(
    after.access(),
    Access::Visible,
    "a board-level block was circumvented by post ownership — the absolute-deny property is gone"
  );

  // 응답만이 아니라 행이 실제로 안 바뀌었는지까지 본다.
  DbState::new(&db)
    .assert_absent("board_posts", &format!("id = {post_id} AND title = 'blocked'"))
    .await;
}
