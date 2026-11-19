//! What a request must carry, and every way what it carries is refused.
//!
//! A credential is checked on every request, and every suite in this repository holds one that was
//! just issued and is therefore always good. So the checks that matter — is it this server's, has
//! it run out, was it revoked, is it even the right kind, and is the account behind it still one we
//! let in — are walked past rather than asked. Each case here asks exactly one of them, and each
//! kind of credential has an admitted case alongside its refusals: without those, a change that
//! refused every request would pass the rest of the file.
//!
//! A refusal is read as its status and the code naming the rule that refused. `401` alone cannot
//! tell a ban from an expired token from a server that has stopped letting anyone in.

use std::net::SocketAddr;

use api::service::api_key::hash_api_key;
use api::service::auth::helper::JwtClaims;
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::prelude::*;
use jsonwebtoken::{EncodingKey, Header};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::json;
use uuid::Uuid;

/// One runtime for every case in this binary.
///
/// The token store the guard consults on every bearer request is a process-wide handle, and its
/// connection belongs to whichever runtime first opened it. A runtime per case — which is what the
/// async test attribute gives — is dropped at the end of the case that opened it, and the next case
/// to present a bearer token is answered by a store whose connection is gone. That answer is a
/// `501`, and it looks nothing like the refusal a case is asking about.
fn runtime() -> &'static tokio::runtime::Runtime {
  static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

  RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("a runtime for the whole binary"))
}

/// A server of this case's own. One per case, because a spawned server lives in the runtime of the
/// test that spawned it and goes down with it.
async fn serving() -> (Mock, SocketAddr, sea_orm::DatabaseConnection) {
  let mock = Mock::new().await;
  let addr = mock.server.addr;
  let db = infra::db::connect().await;

  (mock, addr, db)
}

/// One caller, with a cookie jar of its own.
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

/// What came back from a route that requires an account: the status, and the code naming the rule
/// that refused. An admitted request carries no code and reads as an empty one.
async fn outcome(resp: reqwest::Response) -> (u16, String) {
  let status = resp.status().as_u16();
  let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
  let code = body.get("code").and_then(|code| code.as_str()).unwrap_or_default().to_owned();

  (status, code)
}

/// Presents `credential` in the authorization header and asks for a route that requires an account.
async fn presenting(base: SocketAddr, credential: &str) -> (u16, String) {
  let resp = caller()
    .get(at(base, "/me"))
    .header(reqwest::header::AUTHORIZATION, credential)
    .send()
    .await
    .expect("the /me request");

  outcome(resp).await
}

/// Asks for the same route carrying whatever this caller's jar holds, and nothing else.
async fn carrying_only_the_jar(http: &reqwest::Client, base: SocketAddr) -> (u16, String) {
  outcome(http.get(at(base, "/me")).send().await.expect("the /me request")).await
}

/// Signs in for a token pair. Returns the access and refresh tokens.
async fn token_pair(base: SocketAddr, user: &TestUser) -> (String, String) {
  let resp = caller()
    .post(at(base, "/auth/token"))
    .json(&json!({
      "username": user.model.username,
      "password": user.password,
      "rememberMe": false,
    }))
    .send()
    .await
    .expect("the sign-in request");

  assert_eq!(resp.status().as_u16(), 200, "the fixture's own sign-in must succeed");

  let body: serde_json::Value = resp.json().await.expect("a token pair");
  let read = |key: &str| {
    body
      .get(key)
      .and_then(|token| token.as_str())
      .expect("the pair carries both tokens")
      .to_owned()
  };

  (read("access_token"), read("refresh_token"))
}

/// Signs in for a cookie session, and hands back the caller holding it.
async fn signed_in(base: SocketAddr, user: &TestUser) -> reqwest::Client {
  let http = caller();
  let resp = http
    .post(at(base, "/auth/session"))
    .json(&json!({
      "username": user.model.username,
      "password": user.password,
      "rememberMe": false,
    }))
    .send()
    .await
    .expect("the sign-in request");

  assert_eq!(resp.status().as_u16(), 200, "the fixture's own sign-in must succeed");

  http
}

fn jwt_secret() -> String {
  infra::settings::get()
    .jwt
    .as_ref()
    .and_then(|jwt| jwt.secret.clone())
    .expect("the e2e settings name a jwt secret")
}

/// A token this server would otherwise accept, so that a case can change exactly one thing about it
/// and ask whether that one thing is what refuses it.
fn mint(
  user: &TestUser,
  secret: &str,
  token_type: &str,
  issued_at: chrono::DateTime<Utc>,
  expires_at: chrono::DateTime<Utc>,
) -> String {
  let claims = JwtClaims {
    sub:        user.model.id.to_string(),
    username:   user.model.username.clone(),
    iat:        issued_at.timestamp(),
    exp:        expires_at.timestamp(),
    jti:        Uuid::new_v4().to_string(),
    token_type: token_type.to_owned(),
  };

  jsonwebtoken::encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
    .expect("a minted token")
}

async fn ban(db: &sea_orm::DatabaseConnection, user_id: i32) {
  entity::bans::ActiveModel {
    user_id: Set(Some(user_id)),
    reason: Set(Some("banned".into())),
    created_at: Set(Utc::now()),
    ..Default::default()
  }
  .insert(db)
  .await
  .expect("place the ban");
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

/// Issues an api key for `user_id`, and returns the plaintext the holder would present.
async fn api_key(
  db: &sea_orm::DatabaseConnection,
  user_id: i32,
  expires_at: Option<chrono::DateTime<Utc>>,
) -> String {
  let plaintext = format!("test-key-{}", Uuid::new_v4());

  entity::api_keys::ActiveModel {
    key:        Set(hash_api_key(&plaintext)),
    key_prefix: Set(Some(plaintext.chars().take(8).collect())),
    user_id:    Set(user_id),
    expires_at: Set(expires_at),
    is_secured: Set(false),
    created_at: Set(Utc::now()),
  }
  .insert(db)
  .await
  .expect("issue the key");

  plaintext
}

// ─── a bearer token ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_a_fresh_access_token_is_admitted() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (access, _refresh) = token_pair(base, &user).await;

    assert_eq!(
      presenting(base, &format!("Bearer {access}")).await.0,
      200,
      "a token this server issued a moment ago reaches a route that requires an account"
    );
  });
}

#[test]
fn test_a_request_with_no_credentials_is_refused() {
  runtime().block_on(async {
    let (_mock, base, _db) = serving().await;

    assert_eq!(
      carrying_only_the_jar(&caller(), base).await,
      (401, "E1007".to_owned()),
      "a caller that has never signed in carries nothing, and nothing is not a credential"
    );
  });
}

#[test]
fn test_an_authorization_header_with_no_token_is_refused() {
  runtime().block_on(async {
    let (_mock, base, _db) = serving().await;

    assert_eq!(
      presenting(base, "Bearer").await,
      (401, "E1015".to_owned()),
      "a header that names a scheme and hands over nothing is malformed, not empty-handed"
    );
  });
}

#[test]
fn test_an_unknown_authorization_method_is_refused() {
  runtime().block_on(async {
    let (_mock, base, _db) = serving().await;

    assert_eq!(
      presenting(base, "Basic dXNlcjpwYXNzd29yZA==").await,
      (501, "E1016".to_owned()),
      "a scheme this server does not implement is answered as unimplemented, and never as admitted"
    );
  });
}

#[test]
fn test_a_token_signed_with_another_secret_is_refused() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");

    let now = Utc::now();
    let elsewhere = "a-secret-this-server-has-never-held-000000";
    let forged = mint(&user, elsewhere, "access", now, now + Duration::hours(1));

    assert_eq!(
      presenting(base, &format!("Bearer {forged}")).await,
      (401, "E1011".to_owned()),
      "a token that says all the right things but was signed by somebody else is refused — the \
       signature is the whole of what makes it this server's"
    );
  });
}

#[test]
fn test_an_expired_access_token_is_refused() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");

    let now = Utc::now();
    let expired = mint(&user, &jwt_secret(), "access", now - Duration::hours(2), now - Duration::hours(1));

    assert_eq!(
      presenting(base, &format!("Bearer {expired}")).await,
      (401, "E1011".to_owned()),
      "a token whose term has passed is refused, however properly it is signed"
    );
  });
}

#[test]
fn test_a_refresh_token_is_not_an_access_token() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (_access, refresh) = token_pair(base, &user).await;

    assert_eq!(
      presenting(base, &format!("Bearer {refresh}")).await,
      (401, "E1011".to_owned()),
      "the long-lived half of the pair authenticates nothing — it buys a new pair and that is all"
    );
  });
}

#[test]
fn test_a_logged_out_access_token_is_refused() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (access, refresh) = token_pair(base, &user).await;

    let logout = caller()
      .delete(at(base, "/auth/token"))
      .header(reqwest::header::AUTHORIZATION, format!("Bearer {access}"))
      .json(&json!({ "refresh_token": refresh }))
      .send()
      .await
      .expect("the logout request");
    assert_eq!(logout.status().as_u16(), 200, "the fixture's own logout must succeed");

    assert_eq!(
      presenting(base, &format!("Bearer {access}")).await,
      (401, "E1011".to_owned()),
      "signing out revokes the token in hand — it has not expired, so nothing else would refuse it"
    );
  });
}

#[test]
fn test_a_spent_refresh_token_cannot_be_spent_again() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (_access, refresh) = token_pair(base, &user).await;

    let spend = |token: String| async move {
      caller()
        .post(at(base, "/auth/token/refresh"))
        .json(&json!({ "refresh_token": token }))
        .send()
        .await
        .expect("the refresh request")
    };

    let rotated = spend(refresh.clone()).await;
    assert_eq!(rotated.status().as_u16(), 200, "spending a refresh token buys a new pair");

    let body: serde_json::Value = rotated.json().await.expect("the new pair");
    let issued = body.get("access_token").and_then(|token| token.as_str()).expect("a new access token");
    assert_eq!(
      presenting(base, &format!("Bearer {issued}")).await.0,
      200,
      "and the pair it bought is good"
    );

    assert_eq!(
      outcome(spend(refresh).await).await,
      (401, "E1011".to_owned()),
      "the token that was spent cannot be spent a second time — a replayed refresh is the case \
       rotation exists for"
    );
  });
}

#[test]
fn test_a_ban_reaches_a_token_already_issued() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (access, _refresh) = token_pair(base, &user).await;

    ban(&db, user.model.id).await;

    assert_eq!(
      presenting(base, &format!("Bearer {access}")).await,
      (401, "E1002".to_owned()),
      "a ban raised after the token was issued reaches it on the next request, not at the next sign-in"
    );

    lift(&db, user.model.id).await;
  });
}

#[test]
fn test_disabling_an_account_reaches_a_token_already_issued() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (access, _refresh) = token_pair(base, &user).await;

    disable(&db, user.model.id).await;

    assert_eq!(
      presenting(base, &format!("Bearer {access}")).await,
      (401, "E1001".to_owned()),
      "an account disabled after the token was issued stops being let in at once"
    );
  });
}

// ─── a session cookie ──────────────────────────────────────────────────────────────────────────

#[test]
fn test_a_cookie_session_is_admitted() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;

    assert_eq!(carrying_only_the_jar(&http, base).await.0, 200, "the session reaches the route");
    assert_eq!(
      carrying_only_the_jar(&http, base).await.0,
      200,
      "and goes on reaching it — the account is asked about on every request, and this one is fine"
    );
  });
}

#[test]
fn test_a_ban_reaches_a_cookie_session_already_established() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;

    ban(&db, user.model.id).await;

    assert_eq!(
      carrying_only_the_jar(&http, base).await,
      (401, "E1002".to_owned()),
      "a ban reaches a cookie already in a browser. What the cookie says was true when it was \
       written; whether the account may still be let in is asked of the account, on every request"
    );

    lift(&db, user.model.id).await;
  });
}

#[test]
fn test_disabling_an_account_reaches_a_cookie_session_already_established() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;

    disable(&db, user.model.id).await;

    assert_eq!(
      carrying_only_the_jar(&http, base).await,
      (401, "E1001".to_owned()),
      "and so does disabling the account — a session outliving it would be the account still signed \
       in after it stopped existing to us"
    );
  });
}

// ─── an api key ────────────────────────────────────────────────────────────────────────────────

#[test]
fn test_a_valid_api_key_is_admitted() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let key = api_key(&db, user.model.id, None).await;

    assert_eq!(
      presenting(base, &format!("ApiKey {key}")).await.0,
      200,
      "a key with no term runs until it is deleted"
    );
  });
}

#[test]
fn test_an_unknown_api_key_is_refused() {
  runtime().block_on(async {
    let (_mock, base, _db) = serving().await;

    assert_eq!(
      presenting(base, "ApiKey no-such-key-was-ever-issued").await,
      (401, "E1014".to_owned()),
      "a key nobody issued is not a key"
    );
  });
}

#[test]
fn test_an_expired_api_key_is_refused() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let key = api_key(&db, user.model.id, Some(Utc::now() - Duration::hours(1))).await;

    assert_eq!(
      presenting(base, &format!("ApiKey {key}")).await,
      (401, "E1014".to_owned()),
      "a key whose term has passed is refused — the row is still there, which is exactly why the \
       term has to be read"
    );
  });
}

#[test]
fn test_a_ban_reaches_an_api_key() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let key = api_key(&db, user.model.id, None).await;

    ban(&db, user.model.id).await;

    assert_eq!(
      presenting(base, &format!("ApiKey {key}")).await,
      (401, "E1002".to_owned()),
      "a key is its holder's, and a banned holder does not get in by presenting one"
    );

    lift(&db, user.model.id).await;
  });
}

#[test]
fn test_disabling_the_owner_reaches_an_api_key() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let key = api_key(&db, user.model.id, None).await;

    disable(&db, user.model.id).await;

    assert_eq!(
      presenting(base, &format!("ApiKey {key}")).await,
      (401, "E1001".to_owned()),
      "and a disabled owner's key is refused with them"
    );
  });
}

// ─── a password that has been changed ──────────────────────────────────────────────────────────

#[test]
fn test_changing_the_password_voids_the_credentials_issued_before_it() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");

    let (access, refresh) = token_pair(base, &user).await;
    let browser = signed_in(base, &user).await;

    assert_eq!(
      presenting(base, &format!("Bearer {access}")).await.0,
      200,
      "both are good to begin with"
    );
    assert_eq!(carrying_only_the_jar(&browser, base).await.0, 200, "both are good to begin with");

    // The change is verified by a code the account was sent. Placing it is the fixture; what the case
    // is about is what the change does to everything already handed out.
    entity::user_verifications::ActiveModel {
      user_id:    Set(user.model.id),
      kind:       Set("password-change".to_owned()),
      method:     Set("email".to_owned()),
      code:       Set("123456".to_owned()),
      expires_at: Set(Utc::now() + Duration::minutes(10)),
      created_at: Set(Utc::now()),
    }
    .insert(&db)
    .await
    .expect("place the verification code");

    let changed = caller()
      .post(at(base, "/users/change-password"))
      .json(&json!({
        "username": user.model.username,
        "password": "a-completely-different-password",
        "code": "123456",
      }))
      .send()
      .await
      .expect("the change-password request");
    assert_eq!(changed.status().as_u16(), 200, "the fixture's own password change must succeed");

    assert_eq!(
      presenting(base, &format!("Bearer {access}")).await,
      (401, "E1011".to_owned()),
      "the token issued before the change is void. A password is changed because what was handed out \
       before it may be in somebody else's hands, and that is precisely the token this is"
    );
    assert_eq!(
      outcome(
        caller()
          .post(at(base, "/auth/token/refresh"))
          .json(&json!({ "refresh_token": refresh }))
          .send()
          .await
          .expect("the refresh request")
      )
      .await,
      (401, "E1011".to_owned()),
      "and it cannot buy itself a new pair either"
    );
    assert_eq!(
      carrying_only_the_jar(&browser, base).await,
      (401, "E1007".to_owned()),
      "nor does the session established before the change survive it"
    );

    // A token carries the second it was minted in and nothing finer, so one minted during the second
    // of the change goes with the rest rather than surviving on the rounding. Wait for that second to
    // pass before asking whether the account can hold a credential again.
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;

    let renewed = TestUser {
      model:    user.model.clone(),
      password: "a-completely-different-password".to_owned(),
    };
    let (reissued, _refresh) = token_pair(base, &renewed).await;

    assert_eq!(
      presenting(base, &format!("Bearer {reissued}")).await.0,
      200,
      "what the change voids is what came before it — the account itself is still an account, and \
       the new password still gets in"
    );
  });
}
