//! Who is let in at the door, and every way an attempt is turned away.
//!
//! Signing in is the setup step of every other suite, so a login that starts admitting everybody —
//! or refusing everybody — leaves the rest of them green either way: one of them stops testing
//! authorization and the other stops running. Nothing here is setup. Each case makes one attempt
//! and says what must come back, and the admitted case is not padding — without it a change that
//! refuses the whole world passes every refusal below.
//!
//! A refusal is read as its status **and** the code that says which rule refused. A wrong password
//! and a ban are both `401`, and a suite that cannot tell them apart cannot tell either of them
//! from an outage.

use std::net::SocketAddr;

use chrono::Utc;
use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::json;

/// A server of this case's own. One per case, because a spawned server lives in the runtime of the
/// test that spawned it and goes down with it.
async fn serving() -> (Mock, SocketAddr, sea_orm::DatabaseConnection) {
  let mock = Mock::new().await;
  let addr = mock.server.addr;
  let db = infra::db::connect().await;

  (mock, addr, db)
}

/// One caller, with a cookie jar of its own — an attempt must not be answered by a session another
/// case left behind.
fn caller() -> reqwest::Client {
  reqwest::Client::builder()
    .cookie_store(true)
    .redirect(reqwest::redirect::Policy::none())
    .build()
    .expect("an http client")
}

fn at(base: SocketAddr, path: &str) -> String {
  format!("http://{base}{path}")
}

async fn sign_in(
  http: &reqwest::Client,
  base: SocketAddr,
  path: &str,
  username: &str,
  password: &str,
) -> reqwest::Response {
  http
    .post(at(base, path))
    .json(&json!({ "username": username, "password": password, "rememberMe": false }))
    .send()
    .await
    .expect("the sign-in request")
}

/// What came back, as the pair a refusal is read by: the status, and the code naming the rule that
/// refused. An admitted request carries no code, and reads as an empty one.
async fn outcome(resp: reqwest::Response) -> (u16, String) {
  let status = resp.status().as_u16();
  let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
  let code = body.get("code").and_then(|code| code.as_str()).unwrap_or_default().to_owned();

  (status, code)
}

/// Whether this jar reaches a route that requires an account. The question every refusal has to be
/// asked as well: a refusal that still left a session behind refused nothing.
async fn reaches_me(http: &reqwest::Client, base: SocketAddr) -> bool {
  http.get(at(base, "/me")).send().await.expect("the /me request").status().is_success()
}

async fn ban(db: &sea_orm::DatabaseConnection, user_id: i32) -> i32 {
  entity::bans::ActiveModel {
    user_id: Set(Some(user_id)),
    reason: Set(Some("banned".into())),
    created_at: Set(Utc::now()),
    ..Default::default()
  }
  .insert(db)
  .await
  .expect("place the ban")
  .id
}

async fn lift(db: &sea_orm::DatabaseConnection, user_id: i32) {
  Bans::delete_many()
    .filter(entity::bans::Column::UserId.eq(user_id))
    .exec(db)
    .await
    .expect("clear the ban");
}

async fn disable(db: &sea_orm::DatabaseConnection, user_id: i32) {
  entity::users::ActiveModel {
    id: sea_orm::ActiveValue::Unchanged(user_id),
    is_enabled: Set(false),
    ..Default::default()
  }
  .update(db)
  .await
  .expect("disable the account");
}

#[tokio::test]
async fn test_the_right_password_is_admitted() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");

  let http = caller();
  let resp = sign_in(&http, base, "/auth/session", &user.model.username, &user.password).await;

  assert_eq!(resp.status().as_u16(), 200, "the account's own password is what gets it in");
  assert!(
    reaches_me(&http, base).await,
    "and what it gets is a session that reaches a route requiring one"
  );
}

#[tokio::test]
async fn test_a_wrong_password_is_refused() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");

  let http = caller();
  let resp = sign_in(&http, base, "/auth/session", &user.model.username, "not-the-password").await;

  assert_eq!(
    outcome(resp).await,
    (401, "E1000".to_owned()),
    "a password that is not the account's is refused"
  );
  assert!(!reaches_me(&http, base).await, "and it leaves behind no session that reaches anything");
}

#[tokio::test]
async fn test_an_unknown_username_is_refused() {
  let (_mock, base, _db) = serving().await;

  let http = caller();
  let resp = sign_in(&http, base, "/auth/session", "nobody-by-that-name", "any-password").await;

  assert_eq!(
    outcome(resp).await,
    (401, "E1000".to_owned()),
    "a name that resolves to no account is refused, and refused as the same thing a wrong password \
     is — the answer must not say which of the two it was"
  );
  assert!(!reaches_me(&http, base).await, "and it leaves behind no session that reaches anything");
}

#[tokio::test]
async fn test_a_disabled_account_is_refused() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");
  disable(&db, user.model.id).await;

  let http = caller();
  let resp = sign_in(&http, base, "/auth/session", &user.model.username, &user.password).await;
  let (status, code) = outcome(resp).await;

  assert_eq!(status, 401, "a disabled account is refused its own password");
  assert!(
    matches!(code.as_str(), "E1000" | "E1001"),
    "and refused as an account that cannot sign in — whether the answer names the disabling or \
     keeps it to itself, it is not an admission (got {code})"
  );
  assert!(!reaches_me(&http, base).await, "and it leaves behind no session that reaches anything");
}

#[tokio::test]
async fn test_a_banned_account_is_refused() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");
  ban(&db, user.model.id).await;

  let http = caller();
  let resp = sign_in(&http, base, "/auth/session", &user.model.username, &user.password).await;

  assert_eq!(
    outcome(resp).await,
    (401, "E1002".to_owned()),
    "a ban is what refuses the attempt, and the answer says so — a banned account holding the right \
     password must not be let in"
  );
  assert!(!reaches_me(&http, base).await, "and it leaves behind no session that reaches anything");

  lift(&db, user.model.id).await;
}

#[tokio::test]
async fn test_the_token_endpoint_admits_the_right_password() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");

  let http = caller();
  let resp = sign_in(&http, base, "/auth/token", &user.model.username, &user.password).await;

  assert_eq!(resp.status().as_u16(), 200, "the same password buys a token pair");

  let body: serde_json::Value = resp.json().await.expect("a token pair");
  assert!(
    body.get("access_token").and_then(|token| token.as_str()).is_some_and(|token| !token.is_empty()),
    "and the pair carries an access token: {body}"
  );
}

#[tokio::test]
async fn test_the_token_endpoint_refuses_a_wrong_password() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");

  let http = caller();
  let resp = sign_in(&http, base, "/auth/token", &user.model.username, "not-the-password").await;
  let (status, code) = outcome(resp).await;

  assert_eq!((status, code), (401, "E1000".to_owned()), "no token is issued on a wrong password");
}

#[tokio::test]
async fn test_the_token_endpoint_refuses_a_banned_account() {
  let (_mock, base, db) = serving().await;
  let user = TestUser::create(&db).await.expect("an account");
  ban(&db, user.model.id).await;

  let http = caller();
  let resp = sign_in(&http, base, "/auth/token", &user.model.username, &user.password).await;

  assert_eq!(
    outcome(resp).await,
    (401, "E1002".to_owned()),
    "the token endpoint asks the same question of the same ban as the session endpoint"
  );

  lift(&db, user.model.id).await;
}
