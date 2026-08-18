//! Proves the authorization harness can see what a status-code assertion cannot.
//!
//! These are tests *of the harness*, not of the framework. They exist because the harness makes a
//! strong claim — that it distinguishes a row filtered out by a query's own scope from a request
//! refused by a handler check — and a claim like that is worth nothing unasserted. If these ever
//! stop failing on the negative cases, the harness has gone blind and every suite built on it is
//! reporting false confidence.
//!
//! Requires a real Postgres: the whole point is that the scope under test lives in SQL.

use e2e::prelude::*;
use reqwest::Method;
use sea_orm::EntityTrait;
use serde_json::json;

/// Two callers must be able to hold sessions at the same time.
///
/// The pre-existing `TestClient` keeps one shared cookie jar, so logging in as a second user
/// silently replaced the first — every "user B cannot see user A's row" test written on it was
/// actually asking user B about user B. This pins that `Session` does not have that flaw.
#[tokio::test]
async fn two_sessions_stay_independent() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let alice = TestUser::create(&db).await.expect("alice");
  let bob = TestUser::create(&db).await.expect("bob");

  let alice_s = Session::login(mock.server.addr, &alice, "alice").await;
  let bob_s = Session::login(mock.server.addr, &bob, "bob").await;

  // 두 세션이 서로를 덮어썼다면 둘 다 나중에 로그인한 쪽으로 나온다.
  let a = alice_s.probe_one("/me", &alice.model.id).await;
  let b = bob_s.probe_one("/me", &bob.model.id).await;

  assert_eq!(
    a.body().get("id").and_then(|v| v.as_i64()),
    Some(alice.model.id as i64),
    "alice's session answered as somebody else — the cookie jars are shared"
  );
  assert_eq!(
    b.body().get("id").and_then(|v| v.as_i64()),
    Some(bob.model.id as i64),
    "bob's session answered as somebody else — the cookie jars are shared"
  );
}

/// An unauthenticated caller is a distinct outcome from an authenticated-but-refused one.
///
/// Collapsing the two is how a broken login fixture reads as a passing authorization test: every
/// case "correctly denies" because nobody was ever logged in.
#[tokio::test]
async fn anonymous_is_not_the_same_as_forbidden() {
  let mock = Mock::new().await;
  let anon = Session::anonymous(mock.server.addr);

  let probe = anon.probe_list("/users").await;
  let access = probe.ids();
  assert!(access.is_empty(), "an anonymous caller must not receive rows");

  anon.probe_list("/users").await.assert_refused(Access::Unauthenticated);
}

/// The load-bearing claim: a list that leaks another tenant's row fails, even though the request
/// answered `200 OK`.
///
/// This is the shape of three of the criticals the audit confirmed. A suite that asserts
/// `status == 200` passes on the leak; `assert_exactly` does not.
#[tokio::test]
async fn exact_ids_catch_a_leak_that_a_status_code_misses() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let admin = TestUser::create_admin(&db).await.expect("admin");
  let admin_s = Session::login(mock.server.addr, &admin, "admin").await;

  // 관리자는 전체를 본다. 그 전체에는 방금 만든 다른 사용자가 들어 있다.
  let outsider = TestUser::create(&db).await.expect("outsider");
  let listed = admin_s.probe_list("/users?limit=200").await;

  assert!(
    listed.ids().contains(&outsider.model.id.to_string()),
    "fixture assumption broken: the admin list should contain every user"
  );

  // 같은 응답을 "정확히 나만" 으로 단언하면 반드시 실패해야 한다.
  // 실패하지 않는다면 assert_exactly 가 아무것도 검사하지 않고 있다는 뜻이다.
  let listed = admin_s.probe_list("/users?limit=200").await;
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    listed.assert_exactly(&[&admin.model.id]);
  }));
  assert!(
    result.is_err(),
    "assert_exactly accepted a response containing rows it was not told to expect — the harness is blind"
  );
}

/// `Filtered` and `Forbidden` must not be interchangeable.
///
/// A regression that deletes a list query's scope while leaving a single-row handler check in
/// place keeps answering `403` for `GET /x/{id}` and starts leaking `GET /x`. A test that accepts
/// "denied somehow" stays green through exactly that change.
#[tokio::test]
async fn filtered_and_forbidden_are_distinguished() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("user");
  let session = Session::login(mock.server.addr, &user, "plain member").await;

  // 이 사용자에게는 role 정책이 없으므로 라우트 게이트가 거절한다 — 핸들러 층이다.
  let probe = session.probe_one("/roles/1", &1i32).await;
  let observed = probe.access();

  assert!(
    matches!(observed, Access::Forbidden | Access::NotFound | Access::Filtered),
    "expected the request to be denied in some form, got {observed:?}"
  );

  // 관측된 것과 다른 층을 단언하면 반드시 실패해야 한다.
  let other = match observed {
    Access::Forbidden => Access::Filtered,
    _ => Access::Forbidden,
  };
  let probe = session.probe_one("/roles/1", &1i32).await;
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    probe.assert(other);
  }));
  assert!(
    result.is_err(),
    "assert() accepted {other:?} when the endpoint actually produced {observed:?} — the layer distinction is not being enforced"
  );
}

/// A write that is refused must leave the database alone, and the harness must be able to say so.
///
/// A `403` proves the response, not the state. This pins that the assertion looks at the row.
#[tokio::test]
async fn refused_write_leaves_no_row_behind() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  let user = TestUser::create(&db).await.expect("user");
  let session = Session::login(mock.server.addr, &user, "plain member").await;

  let name = format!("harness-probe-{}", user.model.id);
  session
    .probe_write(Method::POST, "/roles", Some(&json!({ "name": name, "description": "x" })))
    .await;

  DbState::new(&db).assert_absent("roles", &format!("name = '{name}'")).await;
}

/// The organization fixture must build a real tree, deep enough that a hierarchy walk which stops
/// early, or runs the wrong way, is visible.
#[tokio::test]
async fn org_tree_has_the_shape_the_rule_needs() {
  // 이 케이스만 서버가 필요 없어 `Mock::new()` 를 건너뛰는데, 부트스트랩을 여는 것도 그쪽이었다.
  // 그대로 두면 `.env.e2e` 가 실리기 전에 `settings::get()` 에 닿아 `spin::Once` 를 잘못된 값으로
  // 굳혀 버리고, 같은 바이너리의 나머지 다섯 케이스가 그 값을 물려받아 병렬 실행에서 전부 죽는다.
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let tree = OrgTree::create(&db).await.expect("tree");

  let parent_of = |id: i32| async move {
    entity::prelude::Organizations::find_by_id(id)
      .one(&infra::db::connect().await)
      .await
      .unwrap()
      .unwrap()
      .parent_id
  };

  assert_eq!(parent_of(tree.grandchild).await, Some(tree.child), "grandchild hangs off child");
  assert_eq!(parent_of(tree.child).await, Some(tree.subject), "child hangs off subject");
  assert_eq!(parent_of(tree.subject).await, Some(tree.parent), "subject hangs off parent");
  assert_eq!(parent_of(tree.sibling).await, Some(tree.parent), "sibling shares subject's parent");
  assert_eq!(parent_of(tree.aunt).await, Some(tree.root), "aunt hangs off root");
  assert_eq!(parent_of(tree.root).await, None, "root has no parent");

  // 규칙이 요구하는 두 집합이 서로 겹치지 않아야 한다. 겹치면 케이스가 무의미해진다.
  let inside = tree.subject_and_descendants();
  let outside = tree.out_of_scope();
  for i in inside {
    assert!(!outside.contains(&i), "org {i} is in both the in-scope and out-of-scope sets");
  }

  // 깊이 5 사슬 — 두 단계에서 멈추는 탐색과 진짜 재귀를 가르는 자리.
  let chain = OrgTree::chain(&db, tree.subject, 5).await.expect("chain");
  assert_eq!(chain.len(), 5);
  assert_eq!(parent_of(chain[4]).await, Some(chain[3]), "the chain is linked all the way down");
}
