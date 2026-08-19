//! End-to-end board usage/management scenarios.
//!
//! Exercises the behaviours fixed/added on the `bs` branch through the real HTTP stack:
//! - post author lifecycle (create → read → update → delete)
//! - `kind=article` list returns articles, not notices (regression guard)
//! - private content visibility hides a post from non-author/non-admin
//! - password-protected posts reject missing/wrong passwords on both `get` and `view`
//! - reactions: forged `userId` ignored, idempotent, owner-only mutation
//! - file lifecycle: upload→pending, attach→active(refs), orphan reaper, inline-image auto-claim

use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::ap_test_utils::{block_object, grant_object, grant_service};
use e2e::prelude::*;
use entity::bps::boards::{BoardPostContentVisibility, BoardPostRecordVisibility};
use entity::fs::files::FileStatus;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde_json::{Value, json};
use uuid::Uuid;

// ---- helpers ---------------------------------------------------------------

/// 종전 `access_policies` 한 행에 해당하는 부여. 객체가 `service` 면 라우트 게이트,
/// 그 밖이면 그 행 하나에 대한 부여다.
async fn allow(db: &DatabaseConnection, user: &TestUser, ns: &str, object: &str, actions: &[&str]) {
  let actions: Vec<String> = actions.iter().map(|s| (*s).to_string()).collect();
  if object == "service" {
    grant_service(db, user, ns, actions).await.expect("grant the route gate");
  } else {
    grant_object(db, user, ns, object, actions).await.expect("grant the row");
  }
}

/// Grant the bps namespaces a post author needs: `board_post` service-level plus the per-board and
/// per-group object policies the create/read checks require.
async fn grant_post_perms(
  db: &DatabaseConnection,
  user: &TestUser,
  board: &entity::bps::boards::Model,
  actions: &[&str],
) {
  allow(db, user, "board_post", "service", actions).await;
  // `/boards/{id}/posts` is served by the board controller, which gates on `board:read` service-level.
  allow(db, user, "board", "service", actions).await;
  allow(db, user, "board", &board.id.to_string(), actions).await;
  if let Some(group_id) = board.group_id {
    allow(db, user, "board_group", &group_id.to_string(), actions).await;
  }
}

async fn make_board(
  db: &DatabaseConnection,
  content_visibility: BoardPostContentVisibility,
  reaction_visibility: BoardPostRecordVisibility,
  can_upload_file: bool,
) -> entity::bps::boards::Model {
  let group_id = BoardGroups::insert(entity::bps::board_groups::ActiveModel {
    name: Set(format!("g-{}", generate_url_safe_string(6).unwrap())),
    ..Default::default()
  })
  .exec(db)
  .await
  .unwrap()
  .last_insert_id;

  let id = Boards::insert(entity::bps::boards::ActiveModel {
    group_id: Set(Some(group_id)),
    name: Set(format!("b-{}", generate_url_safe_string(6).unwrap())),
    content_visibility: Set(content_visibility),
    reaction_visibility: Set(reaction_visibility),
    can_upload_file: Set(can_upload_file),
    max_file_size: Set(0),
    max_file_count: Set(0),
    is_enabled: Set(true),
    ..Default::default()
  })
  .exec(db)
  .await
  .unwrap()
  .last_insert_id;

  Boards::find_by_id(id).one(db).await.unwrap().unwrap()
}

async fn create_post(mock: &Mock, body: Value) -> reqwest::Response {
  mock.client.post("/board-posts").json(&body).send().await.unwrap()
}

async fn set_viewer_visibility(db: &DatabaseConnection, board_id: i32, v: BoardPostRecordVisibility) {
  Boards::update_many()
    .set(entity::bps::boards::ActiveModel {
      viewer_visibility: Set(v),
      ..Default::default()
    })
    .filter(entity::bps::boards::Column::Id.eq(board_id))
    .exec(db)
    .await
    .unwrap();
}

async fn create_published_article(mock: &Mock, board_id: i32, title: &str) -> i32 {
  let resp = create_post(
    mock,
    json!({
      "boardId": board_id, "title": title, "content": "<p>x</p>",
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "public"
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED, "create published article");
  resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32
}

async fn upload_board_file(mock: &Mock, board_id: i32, filename: &str) -> Uuid {
  let part = reqwest::multipart::Part::bytes(b"PNGDATA".to_vec())
    .file_name(filename.to_string())
    .mime_str("image/png")
    .unwrap();
  let form = reqwest::multipart::Form::new()
    .text("board_id", board_id.to_string())
    .text("metadata", "{}")
    .part("file", part);

  let resp = mock.client.post("/board-files").multipart(form).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED, "board file upload");
  let body: Value = resp.json().await.unwrap();
  Uuid::parse_str(body["uuid"].as_str().unwrap()).unwrap()
}

// ---- scenarios -------------------------------------------------------------

#[tokio::test]
async fn scenario_post_author_lifecycle() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create", "update", "delete"]).await;
  author.login(&mock.client).await;

  // create
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "Hello", "content": "<p>world</p>",
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "public"
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED);
  let post_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  // read
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let body: Value = resp.json().await.unwrap();
  assert_eq!(body["title"], "Hello");
  assert_eq!(body["content"], "<p>world</p>");

  // update
  let resp = mock
    .client
    .patch(&format!("/board-posts/{post_id}"))
    .json(&json!({ "title": "Hello-edited" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // delete
  let resp = mock.client.delete(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // gone
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn scenario_article_list_excludes_notices() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;

  let article_body = json!({
    "boardId": board.id, "title": "ARTICLE", "content": "<p>x</p>",
    "contentType": "text/html", "kind": "article", "status": "published", "contentVisibility": "public"
  });
  let notice_body = json!({
    "boardId": board.id, "title": "NOTICE", "content": "<p>x</p>",
    "contentType": "text/html", "kind": "notice", "status": "published", "contentVisibility": "public"
  });
  assert_eq!(create_post(&mock, article_body).await.status(), StatusCode::CREATED);
  assert_eq!(create_post(&mock, notice_body).await.status(), StatusCode::CREATED);

  // kind=article must contain the article and NOT the notice (regression: used to filter Notice).
  let resp = mock
    .client
    .get(&format!("/boards/{}/posts?kind=article&boardId={}", board.id, board.id))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(items.iter().any(|p| p["title"] == "ARTICLE"), "article list must contain the article");
  assert!(
    !items.iter().any(|p| p["title"] == "NOTICE"),
    "article list must not contain the notice"
  );

  // kind=notice returns the notice.
  let resp = mock
    .client
    .get(&format!("/boards/{}/posts?kind=notice&boardId={}", board.id, board.id))
    .send()
    .await
    .unwrap();
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(items.iter().any(|p| p["title"] == "NOTICE"), "notice list must contain the notice");
}

#[tokio::test]
async fn scenario_private_post_visibility() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  // Public board, private post: the post-level setting must still hide it (effective = most restrictive).
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;

  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "secret", "content": "<p>top secret</p>",
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "private"
    }),
  )
  .await;
  let post_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  // author can read their own private post
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // another user WITH board read perms is still blocked by content visibility (P0-3)
  let mock2 = Mock::new().await;
  let other = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &other, &board, &["read"]).await;
  other.login(&mock2.client).await;
  let resp = mock2.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "private post must be hidden from non-author");

  // ...and it must not surface in the other user's list/search (no title/snippet leak)
  let resp = mock2
    .client
    .get(&format!("/boards/{}/posts?kind=article&boardId={}", board.id, board.id))
    .send()
    .await
    .unwrap();
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(
    !items.iter().any(|p| p["id"].as_i64() == Some(post_id)),
    "private post must not appear in a non-author's list"
  );

  // the author still sees their own private post in the list
  let resp = mock
    .client
    .get(&format!("/boards/{}/posts?kind=article&boardId={}", board.id, board.id))
    .send()
    .await
    .unwrap();
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(
    items.iter().any(|p| p["id"].as_i64() == Some(post_id)),
    "author sees their own private post in the list"
  );

  // admin bypasses visibility
  let mock3 = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&mock3.client).await;
  let resp = mock3.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "admin can read a private post");
}

#[tokio::test]
async fn scenario_password_protected_post() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;

  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "locked", "content": "<p>secret</p>",
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "public", "password": "s3cret"
    }),
  )
  .await;
  let post_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  let mock2 = Mock::new().await;
  let other = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &other, &board, &["read"]).await;
  other.login(&mock2.client).await;

  // GET carries no password → blocked (regression guard for the get() bypass)
  let resp = mock2.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "GET on a password post must be blocked");

  // view with no password → blocked (regression: omitting the password used to pass)
  let resp = mock2
    .client
    .post(&format!("/board-posts/{post_id}/view"))
    .json(&json!({}))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "view without password must be blocked");

  // view with wrong password → blocked
  let resp = mock2
    .client
    .post(&format!("/board-posts/{post_id}/view"))
    .json(&json!({ "password": "nope" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN);

  // view with correct password → ok
  let resp = mock2
    .client
    .post(&format!("/board-posts/{post_id}/view"))
    .json(&json!({ "password": "s3cret" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "correct password unlocks the post");

  // author reads their own password post without supplying a password
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "author bypasses the password gate");
}

#[tokio::test]
async fn scenario_reaction_lifecycle_and_security() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "reactable").await;

  // reactor
  let mock_r = Mock::new().await;
  let reactor = TestUser::create(&db).await.unwrap();
  allow(&db, &reactor, "board_post_reaction", "service", &["read", "create", "update", "delete"]).await;
  reactor.login(&mock_r.client).await;

  // create a reaction with a FORGED userId — the server must use the session user instead
  let resp = mock_r
    .client
    .post("/board-post-reactions")
    .json(&json!({ "postId": post_id, "value": "like", "userId": 999_999 }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  let reaction_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32;

  let row = entity::bps::board_post_reactions::Entity::find_by_id(reaction_id)
    .one(&db)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    row.user_id,
    Some(reactor.model.id),
    "user_id must come from the session, not the client"
  );

  // idempotent: same (post, user, value) returns the same row
  let resp = mock_r
    .client
    .post("/board-post-reactions")
    .json(&json!({ "postId": post_id, "value": "like" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  let reaction_id2 = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32;
  assert_eq!(reaction_id2, reaction_id, "duplicate reaction must be idempotent");

  // a stranger cannot delete someone else's reaction
  let mock_x = Mock::new().await;
  let stranger = TestUser::create(&db).await.unwrap();
  allow(
    &db,
    &stranger,
    "board_post_reaction",
    "service",
    &["read", "create", "update", "delete"],
  )
  .await;
  stranger.login(&mock_x.client).await;
  let resp = mock_x
    .client
    .delete(&format!("/board-post-reactions/{reaction_id}"))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "non-owner cannot delete a reaction");

  // the owner can delete it
  let resp = mock_r
    .client
    .delete(&format!("/board-post-reactions/{reaction_id}"))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "owner deletes their reaction");
}

#[tokio::test]
async fn scenario_file_upload_claim_and_orphan_reap() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, true).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  allow(&db, &author, "board_file", "service", &["read", "create", "delete"]).await;
  author.login(&mock.client).await;

  // upload → pending claim (post_id NULL, refs 0, TTL set)
  let uuid = upload_board_file(&mock, board.id, "a.png").await;
  let bf = BoardFiles::find_by_id(uuid).one(&db).await.unwrap().unwrap();
  assert_eq!(bf.post_id, None, "fresh upload is a pending claim");
  let f = Files::find_by_id(uuid).one(&db).await.unwrap().unwrap();
  assert_eq!(f.refs, 0);
  assert!(f.expires_at.is_some(), "pending file carries a TTL");
  assert_eq!(f.status, FileStatus::Temporary);

  // attach via post create → claimed (post_id set, refs 1, TTL cleared, active)
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "with file", "content": "<p>x</p>",
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "public", "attachedFileUuids": [uuid.to_string()]
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED);
  let post_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32;

  let bf = BoardFiles::find_by_id(uuid).one(&db).await.unwrap().unwrap();
  assert_eq!(bf.post_id, Some(post_id), "claimed file points at the post");
  let f = Files::find_by_id(uuid).one(&db).await.unwrap().unwrap();
  assert_eq!(f.refs, 1);
  assert!(f.expires_at.is_none(), "attached file TTL is cleared");
  assert_eq!(f.status, FileStatus::Active);

  // upload a second file, leave it unattached, force its TTL into the past, then reap
  let orphan = upload_board_file(&mock, board.id, "orphan.png").await;
  Files::update(entity::fs::files::ActiveModel {
    uuid: Unchanged(orphan),
    expires_at: Set(Some((Utc::now() - Duration::hours(1)).fixed_offset())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .unwrap();

  let reaped = api::service::fs::maintenance::reap_orphan_files(&db).await.unwrap();
  assert!(reaped >= 1, "reaper removes the expired unattached file");
  assert!(Files::find_by_id(orphan).one(&db).await.unwrap().is_none(), "orphan file is deleted");
  assert!(Files::find_by_id(uuid).one(&db).await.unwrap().is_some(), "attached file is NOT reaped");
}

#[tokio::test]
async fn scenario_inline_image_auto_claim() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, true).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  allow(&db, &author, "board_file", "service", &["create"]).await;
  author.login(&mock.client).await;

  let uuid = upload_board_file(&mock, board.id, "inline.png").await;

  // reference the file ONLY inline in the body (not in attachedFileUuids)
  let content = format!(r#"<p>see</p><img src="/api/files/{uuid}/inline">"#);
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "inline", "content": content,
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "public"
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED);
  let post_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32;

  // server parsed the body and auto-claimed the inline file
  let bf = BoardFiles::find_by_id(uuid).one(&db).await.unwrap().unwrap();
  assert_eq!(bf.post_id, Some(post_id), "inline-referenced file is auto-claimed");
  let f = Files::find_by_id(uuid).one(&db).await.unwrap().unwrap();
  assert_eq!(f.refs, 1);
  assert_eq!(f.status, FileStatus::Active);
}

#[tokio::test]
async fn scenario_inline_file_serving() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, true).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  allow(&db, &author, "board_file", "service", &["create"]).await;
  author.login(&mock.client).await;

  // a pending upload is inline-servable to its uploader (via the file:read grant on upload)
  let uuid = upload_board_file(&mock, board.id, "pic.png").await;
  let resp = mock.client.get(&format!("/files/{uuid}/inline")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "uploader previews their pending file inline");
  assert_eq!(resp.headers().get("content-type").unwrap(), "image/png");
  assert_eq!(resp.headers().get("content-disposition").unwrap(), "inline");

  // attach it to a PUBLIC post → any authenticated user can serve it inline
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "pub", "content": format!(r#"<img src="/files/{uuid}/inline">"#),
      "contentType": "text/html", "kind": "article", "status": "published", "contentVisibility": "public"
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED);

  let mock2 = Mock::new().await;
  let other = TestUser::create(&db).await.unwrap();
  other.login(&mock2.client).await;
  let resp = mock2.client.get(&format!("/files/{uuid}/inline")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "a public post's inline image is servable to others");

  // a PRIVATE post's attachment is hidden from a non-author
  let priv_uuid = upload_board_file(&mock, board.id, "secret.png").await;
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "priv", "content": format!(r#"<img src="/files/{priv_uuid}/inline">"#),
      "contentType": "text/html", "kind": "article", "status": "published", "contentVisibility": "private"
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED);
  let resp = mock2.client.get(&format!("/files/{priv_uuid}/inline")).send().await.unwrap();
  assert_eq!(
    resp.status(),
    StatusCode::FORBIDDEN,
    "a private post's inline image is hidden from others"
  );
}

#[tokio::test]
async fn scenario_reaction_aggregate_on_post() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "react-agg").await;

  // reactor adds a "like"
  let mock_r = Mock::new().await;
  let reactor = TestUser::create(&db).await.unwrap();
  allow(&db, &reactor, "board_post_reaction", "service", &["read", "create", "delete"]).await;
  reactor.login(&mock_r.client).await;
  let resp = mock_r
    .client
    .post("/board-post-reactions")
    .json(&json!({ "postId": post_id, "value": "like" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  let reaction_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  // the post's reactions aggregate now reflects the like (author/owner sees the reaction columns)
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  let body: Value = resp.json().await.unwrap();
  assert_eq!(body["reactions"]["like"].as_i64(), Some(1), "aggregate reflects the new reaction");

  // removing it clears the aggregate
  mock_r
    .client
    .delete(&format!("/board-post-reactions/{reaction_id}"))
    .send()
    .await
    .unwrap();
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  let body: Value = resp.json().await.unwrap();
  assert!(
    body["reactions"].is_null() || body["reactions"]["like"].is_null(),
    "aggregate cleared after the reaction is removed"
  );
}

#[tokio::test]
async fn scenario_reject_executable_upload() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, true).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  allow(&db, &author, "board_file", "service", &["create"]).await;
  author.login(&mock.client).await;

  // a DOS/PE executable masquerading as a .png must be rejected by the content sniff
  let part = reqwest::multipart::Part::bytes(b"MZ\x90\x00\x03".to_vec())
    .file_name("evil.png".to_string())
    .mime_str("image/png")
    .unwrap();
  let form = reqwest::multipart::Form::new()
    .text("board_id", board.id.to_string())
    .text("metadata", "{}")
    .part("file", part);
  let resp = mock.client.post("/board-files").multipart(form).send().await.unwrap();
  assert!(
    resp.status().is_client_error(),
    "executable content must be rejected: {}",
    resp.status()
  );
}

#[tokio::test]
async fn scenario_scheduled_post_autopublish() {
  use entity::bps::board_posts::{BoardPostContentType, BoardPostKind, BoardPostStatus};

  let _mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;
  let author = TestUser::create(&db).await.unwrap();

  // a scheduled post whose time has already passed
  let post_id = BoardPosts::insert(entity::bps::board_posts::ActiveModel {
    board_id: Set(board.id),
    user_id: Set(Some(author.model.id)),
    kind: Set(BoardPostKind::Article),
    status: Set(BoardPostStatus::Scheduled),
    title: Set("scheduled".into()),
    content_type: Set(BoardPostContentType::Html),
    content_visibility: Set(BoardPostContentVisibility::Public),
    scheduled_at: Set(Some((Utc::now() - Duration::hours(1)).fixed_offset())),
    ..Default::default()
  })
  .exec(&db)
  .await
  .unwrap()
  .last_insert_id;

  let published = api::service::bps::maintenance::publish_scheduled_posts(&db).await.unwrap();
  assert!(published >= 1, "the due scheduled post is published");

  let post = BoardPosts::find_by_id(post_id).one(&db).await.unwrap().unwrap();
  assert_eq!(post.status, BoardPostStatus::Published);
  assert!(post.published_at.is_some(), "published_at is stamped");
}

#[tokio::test]
async fn scenario_report_lifecycle() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "reportable").await;

  // a reporter files a report on a visible post
  let mock_r = Mock::new().await;
  let reporter = TestUser::create(&db).await.unwrap();
  allow(&db, &reporter, "board_post_report", "service", &["create"]).await;
  reporter.login(&mock_r.client).await;
  let resp = mock_r
    .client
    .post("/board-post-reports")
    .json(&json!({ "postId": post_id, "reason": "spam" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  let report_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  // re-report updates the reason in place (one report per user/post)
  let resp = mock_r
    .client
    .post("/board-post-reports")
    .json(&json!({ "postId": post_id, "reason": "abuse" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.json::<Value>().await.unwrap()["id"].as_i64(), Some(report_id), "report is deduped");

  // a non-moderator cannot list reports
  let resp = mock_r.client.get("/board-post-reports").send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "non-moderator cannot read reports");

  // a moderator lists, sees it, and resolves (deletes) it
  let mock_m = Mock::new().await;
  let moderator = TestUser::create(&db).await.unwrap();
  allow(&db, &moderator, "board_post_report", "service", &["read", "delete"]).await;
  moderator.login(&mock_m.client).await;
  let resp = mock_m
    .client
    .get(&format!("/board-post-reports?postId={post_id}"))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(items.iter().any(|r| r["id"].as_i64() == Some(report_id)), "moderator sees the report");

  let resp = mock_m.client.delete(&format!("/board-post-reports/{report_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "moderator resolves the report");
}

#[tokio::test]
async fn scenario_viewer_list_visibility() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;
  set_viewer_visibility(&db, board.id, BoardPostRecordVisibility::Public).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "viewed").await;

  // a viewer reads the post (records a view marker)
  let mock_v = Mock::new().await;
  let viewer = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &viewer, &board, &["read"]).await;
  viewer.login(&mock_v.client).await;
  let resp = mock_v
    .client
    .post(&format!("/board-posts/{post_id}/view"))
    .json(&json!({}))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // viewer_visibility=public: the viewer list is readable and contains the viewer
  let resp = mock_v.client.get(&format!("/board-posts/{post_id}/viewers")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(
    items.iter().any(|x| x["userId"].as_i64() == Some(viewer.model.id as i64)),
    "viewer appears in the public viewer list"
  );

  // viewer_visibility=private: hidden from non-admin, visible to admin
  set_viewer_visibility(&db, board.id, BoardPostRecordVisibility::Private).await;
  let resp = mock_v.client.get(&format!("/board-posts/{post_id}/viewers")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "private viewer list hidden from non-admin");

  let mock_a = Mock::new().await;
  TestUser::create_admin(&db).await.unwrap().login(&mock_a.client).await;
  let resp = mock_a.client.get(&format!("/board-posts/{post_id}/viewers")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "admin reads the private viewer list");
}

#[tokio::test]
async fn scenario_soft_delete_persists_and_hides() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create", "delete"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "to-delete").await as i32;

  let resp = mock.client.delete(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // gone from reads
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::NOT_FOUND, "soft-deleted post is not readable");
  let resp = mock
    .client
    .get(&format!("/boards/{}/posts?kind=article&boardId={}", board.id, board.id))
    .send()
    .await
    .unwrap();
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  assert!(
    !items.iter().any(|p| p["id"].as_i64() == Some(post_id as i64)),
    "soft-deleted post is excluded from list"
  );

  // but the row persists, marked deleted
  let row = BoardPosts::find_by_id(post_id).one(&db).await.unwrap().unwrap();
  assert!(row.deleted_at.is_some(), "deleted_at is stamped");
  assert_eq!(row.status, entity::bps::board_posts::BoardPostStatus::Deleted);
}

#[tokio::test]
async fn scenario_attachment_uri_is_gated() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, true).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  allow(&db, &author, "board_file", "service", &["create"]).await;
  author.login(&mock.client).await;

  let uuid = upload_board_file(&mock, board.id, "doc.png").await;
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "title": "with file", "content": "<p>x</p>",
      "contentType": "text/html", "kind": "article", "status": "published",
      "contentVisibility": "public", "attachedFileUuids": [uuid.to_string()]
    }),
  )
  .await;
  let post_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  // the post's attached file exposes the gated read-only path, never the raw storage URI
  let resp = mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap();
  let body: Value = resp.json().await.unwrap();
  let uri = body["attachedFiles"][0]["uri"].as_str().unwrap();
  assert_eq!(uri, format!("/files/{uuid}/inline"), "attachment uri is the gated inline path");
}

#[tokio::test]
async fn scenario_soft_delete_cascades_to_replies() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create", "delete"]).await;
  author.login(&mock.client).await;

  let post_id = create_published_article(&mock, board.id, "parent").await;

  // a comment under the post (child in the parent_id tree)
  let resp = create_post(
    &mock,
    json!({
      "boardId": board.id, "parentId": post_id, "title": "re", "content": "<p>c</p>",
      "contentType": "text/html", "kind": "comment", "status": "published", "contentVisibility": "public"
    }),
  )
  .await;
  assert_eq!(resp.status(), StatusCode::CREATED);
  let child_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32;

  // delete the parent
  let resp = mock.client.delete(&format!("/board-posts/{post_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // both parent and reply are gone from reads
  assert_eq!(
    mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap().status(),
    StatusCode::NOT_FOUND
  );
  assert_eq!(
    mock.client.get(&format!("/board-posts/{child_id}")).send().await.unwrap().status(),
    StatusCode::NOT_FOUND,
    "reply is hidden when its parent is soft-deleted"
  );

  // the reply row is soft-deleted in the DB
  let child = BoardPosts::find_by_id(child_id).one(&db).await.unwrap().unwrap();
  assert!(child.deleted_at.is_some(), "reply is cascade soft-deleted");
}

#[tokio::test]
async fn scenario_board_soft_delete_hides_board_and_posts() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create", "delete"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "p").await;

  // soft-delete the board
  let resp = mock.client.delete(&format!("/boards/{}", board.id)).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // board is hidden, and its posts are hidden too
  assert_eq!(
    mock.client.get(&format!("/boards/{}", board.id)).send().await.unwrap().status(),
    StatusCode::NOT_FOUND,
    "soft-deleted board is not readable"
  );
  assert_eq!(
    mock.client.get(&format!("/board-posts/{post_id}")).send().await.unwrap().status(),
    StatusCode::NOT_FOUND,
    "posts under a soft-deleted board are hidden"
  );

  // the board row persists, marked deleted
  let row = Boards::find_by_id(board.id).one(&db).await.unwrap().unwrap();
  assert!(row.deleted_at.is_some(), "board deleted_at is stamped");
}

/// 종전 `effect = 'deny'` 한 행에 해당하는 차단.
///
/// 개별 부여에 붙는 부정은 사라졌고(설계 §3), 그 자리를 정의가 선언한 `blocked` 관계가
/// 대신한다 — 규칙에서 합집합 **바깥**으로 빠지므로 작성자 소유권으로 우회할 수 없다는 성질은
/// 그대로다. 액션별 차단은 표현할 수 없으므로 인자에서 사라졌다: `blocked` 는 그것을 이름으로
/// 부르는 모든 권한에서 빠진다.
async fn deny(db: &DatabaseConnection, user: &TestUser, ns: &str, object: &str) {
  block_object(db, user, ns, object).await.expect("write the block");
}

#[tokio::test]
async fn scenario_global_deny_blocks_owner() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  // board_post:update service to reach the PATCH handler; the per-post owner grant is auto-created on create.
  allow(&db, &author, "board_post", "service", &["update"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "mine").await;

  // baseline: the author can update their own post (owner branch allows, no deny)
  let resp = mock
    .client
    .patch(&format!("/board-posts/{post_id}"))
    .json(&json!({ "title": "v2" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);

  // a board:update DENY blocks even the author (global-deny: a deny on any branch is absolute)
  deny(&db, &author, "board", &board.id.to_string()).await;
  let resp = mock
    .client
    .patch(&format!("/board-posts/{post_id}"))
    .json(&json!({ "title": "v3" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "global deny blocks the owner too");
}

#[tokio::test]
async fn scenario_operator_moderates_then_global_deny() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;
  let post_id = create_published_article(&mock, board.id, "authored").await;

  // moderator (not the author) with operator perms: board_group + board update on this board
  let mock_m = Mock::new().await;
  let moderator = TestUser::create(&db).await.unwrap();
  allow(&db, &moderator, "board_post", "service", &["update"]).await;
  allow(&db, &moderator, "board", &board.id.to_string(), &["update"]).await;
  if let Some(group_id) = board.group_id {
    allow(&db, &moderator, "board_group", &group_id.to_string(), &["update"]).await;
  }
  moderator.login(&mock_m.client).await;

  // operator can moderate someone else's post
  let resp = mock_m
    .client
    .patch(&format!("/board-posts/{post_id}"))
    .json(&json!({ "title": "moderated" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "operator moderates others' posts");

  // a board:update DENY on the moderator blocks the operator path too (global-deny)
  deny(&db, &moderator, "board", &board.id.to_string()).await;
  let resp = mock_m
    .client
    .patch(&format!("/board-posts/{post_id}"))
    .json(&json!({ "title": "x" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "global deny blocks the operator too");
}

/// Percent-encodes a condition tree for the `where` parameter, the way a client would.
fn encoded(tree: &Value) -> String {
  tree
    .to_string()
    .bytes()
    .map(|byte| match byte {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
        String::from(byte as char)
      }
      other => format!("%{other:02X}"),
    })
    .collect()
}

async fn list_titles(mock: &Mock, query: &str) -> Vec<String> {
  let resp = mock.client.get(&format!("/board-posts?{query}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "the list must answer: {query}");

  resp.json::<Value>().await.unwrap()["items"]
    .as_array()
    .cloned()
    .unwrap_or_default()
    .iter()
    .map(|post| post["title"].as_str().unwrap_or_default().to_string())
    .collect()
}

/// Full text under a condition tree: the same tsvector, and the snippet still driven by the flat
/// parameter alone.
///
/// The three things that have to hold together are the tsquery predicate, the `ts_headline`
/// snippet, and the integrated `search` field being absent from the tree — a field whose predicate
/// spans a title `ILIKE` unioned with a body tsquery reduces to no single expression, so a tree
/// has nothing to compile for it.
#[tokio::test]
async fn scenario_full_text_under_a_condition_tree() {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;
  let board =
    make_board(&db, BoardPostContentVisibility::Public, BoardPostRecordVisibility::Public, false).await;

  let author = TestUser::create(&db).await.unwrap();
  grant_post_perms(&db, &author, &board, &["read", "create"]).await;
  author.login(&mock.client).await;

  let marker = generate_url_safe_string(8).unwrap().to_lowercase();
  let alpha = format!("alpha-{marker}");
  let beta = format!("beta-{marker}");

  for (title, body) in [(&alpha, "the quick brown fox"), (&beta, "a lazy dog sleeps")] {
    let resp = create_post(
      &mock,
      json!({
        "boardId": board.id, "title": title, "content": format!("<p>{body} {marker}</p>"),
        "contentType": "text/html", "kind": "article", "status": "published",
        "contentVisibility": "public"
      }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED, "create the post");
  }

  // The flat full-text parameter reaches the tsvector and nothing else.
  let titles = list_titles(&mock, &format!("text=brown+{marker}")).await;
  assert_eq!(titles, vec![alpha.clone()], "the flat full-text parameter selects one post");

  // The snippet is still built, and still from the flat parameter.
  let resp = mock
    .client
    .get(&format!("/board-posts?with_snippet=true&search=brown+{marker}"))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  let post = items.iter().find(|post| post["title"] == alpha.as_str()).expect("the post is listed");
  assert!(
    post["snippet"].as_str().unwrap_or_default().contains("<mark>"),
    "the flat parameter must still highlight: {post}"
  );

  // A `matches` leaf reaches the same tsvector.
  let tree = json!({ "type": "field", "key": "text", "op": "matches", "value": format!("brown {marker}") });
  let titles = list_titles(&mock, &format!("where={}", encoded(&tree))).await;
  assert_eq!(titles, vec![alpha.clone()], "a `matches` leaf selects the same post");

  // A row matched only through the tree comes back without a snippet — the tree contributes
  // nothing to it, so there is no query for `ts_headline` to have been built from.
  let resp = mock
    .client
    .get(&format!("/board-posts?with_snippet=true&where={}", encoded(&tree)))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let items = resp.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default();
  let post = items.iter().find(|post| post["title"] == alpha.as_str()).expect("the post is listed");
  assert!(post["snippet"].is_null(), "the tree must not drive the snippet: {post}");

  // An `any` group mixes the tsvector with a substring match on the title.
  let tree = json!({
    "type": "any",
    "of": [
      { "type": "field", "key": "text", "op": "matches", "value": format!("brown {marker}") },
      { "type": "field", "key": "title", "op": "contains", "value": beta.clone() },
    ]
  });
  let mut titles = list_titles(&mock, &format!("where={}", encoded(&tree))).await;
  titles.sort();
  assert_eq!(titles, vec![alpha.clone(), beta.clone()], "both branches of the group hold");

  // The integrated `search` field spans two mechanisms and is refused from a tree; its flat
  // parameter is untouched.
  let tree = json!({ "type": "field", "key": "search", "op": "matches", "value": marker.clone() });
  let resp = mock
    .client
    .get(&format!("/board-posts?where={}", encoded(&tree)))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "a custom field has nothing to compile");

  let titles = list_titles(&mock, &format!("search={marker}")).await;
  assert_eq!(titles.len(), 2, "the flat integrated search still spans both posts");
}
