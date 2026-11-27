//! The ways an account is taken over from a foothold smaller than its password, each one refused.
//!
//! Every case here starts from something an attacker can plausibly hold — an account's username, a
//! leaked API key that was meant to carry a few grants, a cookie or token copied for a moment, a
//! refresh token — and asks one route to turn it into more than it is. Each case also has the
//! admitted half beside the refusal, so a change that refused everything would not pass the file.
//!
//! A refusal is read as its status and the code naming the rule that refused.

use std::net::SocketAddr;

use api::authz::write::SubjectKey;
use api::service::api_key::hash_api_key;
use chrono::{Duration, Utc};
use e2e::ap_test_utils::tuple;
use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde_json::json;
use uuid::Uuid;

/// One runtime for every case in this binary: the token store the guard consults on every bearer
/// request is a process-wide handle whose connection belongs to the runtime that opened it.
fn runtime() -> &'static tokio::runtime::Runtime {
  static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

  RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("a runtime for the whole binary"))
}

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

async fn outcome(resp: reqwest::Response) -> (u16, String) {
  let status = resp.status().as_u16();
  let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
  let code = body.get("code").and_then(|code| code.as_str()).unwrap_or_default().to_owned();

  (status, code)
}

async fn signed_in(base: SocketAddr, user: &TestUser) -> reqwest::Client {
  let http = caller();
  let resp = http
    .post(at(base, "/auth/session"))
    .json(&json!({ "username": user.model.username, "password": user.password, "rememberMe": false }))
    .send()
    .await
    .expect("the sign-in request");
  assert_eq!(resp.status().as_u16(), 200, "the fixture's own sign-in must succeed");

  http
}

async fn token_pair(base: SocketAddr, user: &TestUser) -> (String, String) {
  let resp = caller()
    .post(at(base, "/auth/token"))
    .json(&json!({ "username": user.model.username, "password": user.password, "rememberMe": false }))
    .send()
    .await
    .expect("the sign-in request");
  assert_eq!(resp.status().as_u16(), 200, "the fixture's own sign-in must succeed");

  let body: serde_json::Value = resp.json().await.expect("a token pair");
  let read = |key: &str| body[key].as_str().expect("the pair carries both tokens").to_owned();

  (read("access_token"), read("refresh_token"))
}

/// Issues an api key for `user_id`, and returns the plaintext the holder would present.
async fn api_key(db: &sea_orm::DatabaseConnection, user_id: i32, secured: bool) -> String {
  let plaintext = format!("test-key-{}", Uuid::new_v4());

  entity::api_keys::ActiveModel {
    key:        Set(hash_api_key(&plaintext)),
    key_prefix: Set(Some(plaintext.chars().take(8).collect())),
    user_id:    Set(user_id),
    expires_at: Set(None),
    is_secured: Set(secured),
    created_at: Set(Utc::now()),
  }
  .insert(db)
  .await
  .expect("issue the key");

  plaintext
}

/// Places a verification code, as the mail that carries it would have.
async fn place_code(db: &sea_orm::DatabaseConnection, user_id: i32, kind: &str, code: &str) {
  entity::user_verifications::ActiveModel {
    user_id:    Set(user_id),
    kind:       Set(kind.to_owned()),
    method:     Set("email".to_owned()),
    code:       Set(code.to_owned()),
    expires_at: Set(Utc::now() + Duration::minutes(10)),
    created_at: Set(Utc::now()),
  }
  .insert(db)
  .await
  .expect("place the verification code");
}

async fn change_password(base: SocketAddr, user: &TestUser, code: &str) -> (u16, String) {
  outcome(
    caller()
      .post(at(base, "/users/change-password"))
      .json(&json!({ "username": user.model.username, "password": "taken-over-password", "code": code }))
      .send()
      .await
      .expect("the change-password request"),
  )
  .await
}

// ─── verification codes ────────────────────────────────────────────────────────────────────────

#[test]
fn test_a_verification_code_is_spent_by_the_guesses_it_allows() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    place_code(&db, user.model.id, "password-change", "123456").await;

    // Four wrong guesses leave the code standing; the fifth spends it.
    for guess in ["000000", "000001", "000002", "000003", "000004"] {
      assert_eq!(
        change_password(base, &user, guess).await,
        (401, "E3006".to_owned()),
        "a wrong code is refused"
      );
    }

    assert_eq!(
      change_password(base, &user, "123456").await,
      (401, "E3006".to_owned()),
      "the right code after the allowed guesses is refused too: the code was spent by the guess that \
       used up its last chance, so a six-digit code cannot be searched"
    );
    let left = UserVerifications::find()
      .filter(entity::user_verifications::Column::UserId.eq(user.model.id))
      .count(&db)
      .await
      .expect("count the codes");
    assert_eq!(left, 0, "the spent code is gone from the store");
  });
}

#[test]
fn test_the_right_code_within_the_allowed_guesses_is_admitted() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    place_code(&db, user.model.id, "password-change", "654321").await;

    assert_eq!(change_password(base, &user, "000000").await.0, 401);
    assert_eq!(
      change_password(base, &user, "654321").await.0,
      200,
      "the owner who mistyped once still gets in with the code they were sent"
    );
  });
}

#[test]
fn test_a_code_for_a_channel_that_sends_nothing_is_never_stored() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");

    let resp = caller()
      .post(at(base, "/user-verifications"))
      .json(&json!({ "userId": user.model.id, "kind": "password-change", "method": "carrier-pigeon" }))
      .send()
      .await
      .expect("the send request");
    assert_eq!(resp.status().as_u16(), 201, "the answer says nothing about what happened");

    let stored = UserVerifications::find()
      .filter(entity::user_verifications::Column::UserId.eq(user.model.id))
      .count(&db)
      .await
      .expect("count the codes");
    assert_eq!(
      stored, 0,
      "a code nobody receives is one only a guesser can spend, and its owner is never told it exists"
    );
  });
}

// ─── header credentials ────────────────────────────────────────────────────────────────────────

#[test]
fn test_an_api_key_never_leaves_a_cookie_behind() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let key = api_key(&db, user.model.id, false).await;
    let http = caller();

    let presented = http
      .get(at(base, "/me"))
      .header(reqwest::header::AUTHORIZATION, format!("ApiKey {key}"))
      .send()
      .await
      .expect("the /me request");
    assert_eq!(presented.status().as_u16(), 200, "the key itself is admitted");

    ApiKeys::delete_many()
      .filter(entity::api_keys::Column::UserId.eq(user.model.id))
      .exec(&db)
      .await
      .expect("revoke the key");

    assert_eq!(
      outcome(http.get(at(base, "/me")).send().await.expect("the /me request")).await,
      (401, "E1007".to_owned()),
      "with the key revoked, whatever the jar caught from the key's request admits nothing: a key is \
       checked on every request, and a cookie minted from it would outlive the revocation"
    );
  });
}

#[test]
fn test_a_bearer_token_never_leaves_a_cookie_behind() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (access, _refresh) = token_pair(base, &user).await;
    let http = caller();

    let presented = http
      .get(at(base, "/me"))
      .header(reqwest::header::AUTHORIZATION, format!("Bearer {access}"))
      .send()
      .await
      .expect("the /me request");
    assert_eq!(presented.status().as_u16(), 200, "the token itself is admitted");

    assert_eq!(
      outcome(http.get(at(base, "/me")).send().await.expect("the /me request")).await,
      (401, "E1007".to_owned()),
      "the token's request established nothing that outlives it — a cookie minted from it would survive \
       the token's revocation"
    );
  });
}

#[test]
fn test_a_secured_key_of_an_administrator_is_judged_as_the_key_alone() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let admin = TestUser::create_admin(&db).await.expect("an administrator");
    let unsecured = api_key(&db, admin.model.id, false).await;
    let secured = api_key(&db, admin.model.id, true).await;

    let list_users = |key: String| async move {
      outcome(
        caller()
          .get(at(base, "/users"))
          .header(reqwest::header::AUTHORIZATION, format!("ApiKey {key}"))
          .send()
          .await
          .expect("the /users request"),
      )
      .await
    };

    assert_eq!(list_users(unsecured).await.0, 200, "an unsecured key is its owner");
    assert_eq!(
      list_users(secured.clone()).await,
      (403, "E1009".to_owned()),
      "a secured key granted nothing passes no gate, whatever its owner may do"
    );
    assert_eq!(
      outcome(
        caller()
          .get(at(base, "/dashboard/auth"))
          .header(reqwest::header::AUTHORIZATION, format!("ApiKey {secured}"))
          .send()
          .await
          .expect("the dashboard request"),
      )
      .await,
      (401, "E1017".to_owned()),
      "nor is it an administrator because its owner is one"
    );
  });
}

#[test]
fn test_a_secured_key_cannot_reach_its_owners_administrative_power() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let admin = TestUser::create_admin(&db).await.expect("an administrator");
    let victim = TestUser::create(&db).await.expect("another account");
    let secured = api_key(&db, admin.model.id, true).await;
    let holder = SubjectKey::apikey(&hash_api_key(&secured));
    tuple(&db, "service", "user", "update", holder.clone())
      .await
      .expect("the key's update gate");
    tuple(&db, "service", "api_key", "create", holder).await.expect("the key's create gate");

    let as_key = |builder: reqwest::RequestBuilder| {
      builder.header(reqwest::header::AUTHORIZATION, format!("ApiKey {secured}"))
    };

    assert_eq!(
      outcome(
        as_key(caller().patch(at(base, &format!("/users/{}", victim.model.id))))
          .json(&json!({ "isAdmin": true, "password": "taken-over" }))
          .send()
          .await
          .expect("the update request"),
      )
      .await,
      (403, "E1008".to_owned()),
      "the key's update gate reaches only the rows the key may edit, and its owner's flag adds none"
    );
    assert_eq!(
      outcome(
        as_key(caller().post(at(base, &format!("/users/{}/impersonate", victim.model.id))))
          .send()
          .await
          .expect("the impersonate request"),
      )
      .await,
      (401, "E1017".to_owned()),
      "impersonation is a platform administrator's, and the key is not one"
    );
    assert_eq!(
      outcome(
        as_key(caller().post(at(base, "/api-keys")))
          .json(&json!({ "userId": admin.model.id, "isSecured": false }))
          .send()
          .await
          .expect("the key request"),
      )
      .await,
      (403, "E1008".to_owned()),
      "a key carrying only its own grants cannot mint one that carries its owner's whole identity"
    );

    let reread = Users::find_by_id(victim.model.id).one(&db).await.unwrap().unwrap();
    assert!(!reread.is_admin, "the other account was not promoted");
  });
}

#[test]
fn test_an_account_manager_cannot_take_over_an_administrator() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let admin = TestUser::create_admin(&db).await.expect("an administrator");
    let managed = TestUser::create(&db).await.expect("an account the manager reaches");
    let manager = TestUser::create(&db).await.expect("a user manager");
    let holder = SubjectKey::user(manager.model.id);
    tuple(&db, "service", "user", "update", holder.clone()).await.expect("the update gate");
    tuple(&db, "service", "user", "op_update", holder).await.expect("every account's edit");
    let http = signed_in(base, &manager).await;

    assert_eq!(
      outcome(
        http
          .patch(at(base, &format!("/users/{}", admin.model.id)))
          .json(&json!({ "password": "taken-over" }))
          .send()
          .await
          .expect("the update request"),
      )
      .await,
      (403, "E1008".to_owned()),
      "editing every account does not reach an administrator's password"
    );
    let resp = http
      .patch(at(base, &format!("/users/{}", managed.model.id)))
      .json(&json!({ "fullName": "renamed" }))
      .send()
      .await
      .expect("the update request");
    assert_eq!(resp.status().as_u16(), 200, "while the accounts it is meant for are still reached");

    let reread = Users::find_by_id(admin.model.id).one(&db).await.unwrap().unwrap();
    assert_eq!(reread.password, admin.model.password, "the administrator's password stands");
  });
}

#[test]
fn test_changing_what_an_account_may_do_puts_out_its_sessions() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let admin = TestUser::create_admin(&db).await.expect("an administrator");
    let changed = TestUser::create(&db).await.expect("the account that is changed");
    let untouched = TestUser::create(&db).await.expect("an account left as it is");
    let changed_http = signed_in(base, &changed).await;
    let untouched_http = signed_in(base, &untouched).await;
    let admin_http = signed_in(base, &admin).await;

    assert_eq!(changed_http.get(at(base, "/me")).send().await.unwrap().status().as_u16(), 200);

    let resp = admin_http
      .patch(at(base, &format!("/users/{}", changed.model.id)))
      .json(&json!({ "isAdmin": true }))
      .send()
      .await
      .expect("the update request");
    assert_eq!(resp.status().as_u16(), 200, "an administrator may promote an account");

    assert_eq!(
      outcome(changed_http.get(at(base, "/me")).send().await.unwrap()).await,
      (401, "E1007".to_owned()),
      "the session established before the change carries what the account was, so it is void"
    );
    assert_eq!(
      untouched_http.get(at(base, "/me")).send().await.unwrap().status().as_u16(),
      200,
      "and only that account's sessions are"
    );
  });
}

// ─── /me ───────────────────────────────────────────────────────────────────────────────────────

#[test]
fn test_changing_the_password_through_me_takes_the_current_one() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;
    let (access, _refresh) = token_pair(base, &user).await;
    let email = user.model.email.clone();

    let patch = |body: serde_json::Value| http.patch(at(base, "/me")).json(&body).send();

    assert_eq!(
      outcome(patch(json!({ "password": "taken-over", "email": email })).await.unwrap()).await,
      (401, "E3006".to_owned()),
      "a cookie held for a moment is not enough to set a new password"
    );
    assert_eq!(
      outcome(patch(json!({ "email": "attacker@example.com" })).await.unwrap()).await,
      (401, "E3006".to_owned()),
      "nor to move the address the reset codes go to"
    );
    assert_eq!(
      outcome(
        patch(json!({ "password": "taken-over", "email": email, "currentPassword": "not-it" }))
          .await
          .unwrap()
      )
      .await,
      (401, "E3006".to_owned()),
      "a wrong current password is refused"
    );

    assert_eq!(
      patch(json!({ "password": "a-new-password", "email": email, "currentPassword": user.password }))
        .await
        .unwrap()
        .status()
        .as_u16(),
      200,
      "the owner who knows the current password changes it"
    );
    assert_eq!(
      http.get(at(base, "/me")).send().await.unwrap().status().as_u16(),
      200,
      "the session that made the change stays signed in"
    );
    assert_eq!(
      outcome(
        caller()
          .get(at(base, "/me"))
          .header(reqwest::header::AUTHORIZATION, format!("Bearer {access}"))
          .send()
          .await
          .unwrap()
      )
      .await,
      (401, "E1011".to_owned()),
      "and a token handed out before the change is void, as after any reset"
    );
  });
}

// ─── refresh rotation ──────────────────────────────────────────────────────────────────────────

#[test]
fn test_a_refresh_token_presented_at_once_buys_one_pair() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let (_access, refresh) = token_pair(base, &user).await;

    let attempts = (0..8).map(|_| {
      let refresh = refresh.clone();
      async move {
        caller()
          .post(at(base, "/auth/token/refresh"))
          .json(&json!({ "refresh_token": refresh }))
          .send()
          .await
          .expect("the refresh request")
          .status()
          .as_u16()
      }
    });
    let statuses = futures::future::join_all(attempts).await;

    assert_eq!(
      statuses.iter().filter(|status| **status == 200).count(),
      1,
      "one refresh token is spent on exactly one pair, however many requests present it together: {statuses:?}"
    );
  });
}

// ─── administrator routes ──────────────────────────────────────────────────────────────────────

#[test]
fn test_an_administrator_cookie_is_put_out_by_a_reset() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let admin = TestUser::create_admin(&db).await.expect("an administrator");
    let http = signed_in(base, &admin).await;

    assert_eq!(http.get(at(base, "/dashboard/auth")).send().await.unwrap().status().as_u16(), 200);

    entity::users::ActiveModel {
      id: sea_orm::ActiveValue::Unchanged(admin.model.id),
      credentials_reset_at: Set(Some(Utc::now())),
      ..Default::default()
    }
    .update(&db)
    .await
    .expect("reset the credentials");

    assert_eq!(
      outcome(http.get(at(base, "/dashboard/auth")).send().await.unwrap()).await,
      (401, "E1007".to_owned()),
      "a reset voids every cookie issued before it, on the administrator's routes as on every other"
    );
  });
}

// ─── sockets ───────────────────────────────────────────────────────────────────────────────────

/// Opens the `/user` namespace over the polling transport and answers the namespace packet the
/// server sent back: `40` is a connection admitted, `44` one refused.
async fn open_user_socket(http: &reqwest::Client, base: SocketAddr, origin: Option<&str>) -> String {
  let with_origin = |builder: reqwest::RequestBuilder| match origin {
    Some(origin) => builder.header(reqwest::header::ORIGIN, origin),
    None => builder,
  };

  let handshake = with_origin(http.get(at(base, "/socket.io/?EIO=4&transport=polling")))
    .send()
    .await
    .expect("the engine handshake")
    .text()
    .await
    .expect("the handshake body");
  let open: serde_json::Value =
    serde_json::from_str(handshake.trim_start_matches('0')).expect("an engine open packet");
  let sid = open["sid"].as_str().expect("an engine session id").to_owned();
  let polling = at(base, &format!("/socket.io/?EIO=4&transport=polling&sid={sid}"));

  with_origin(http.post(&polling))
    .body("40/user,")
    .send()
    .await
    .expect("the namespace connect");

  // The server's answer arrives on a later poll; a ping met on the way is answered so the
  // connection stays up while the namespace packet is waited for.
  for _ in 0..5 {
    let answer = tokio::time::timeout(
      std::time::Duration::from_secs(15),
      with_origin(http.get(&polling)).send().await.expect("the poll").text(),
    )
    .await
    .expect("the server answers within a ping interval")
    .expect("the poll body");

    for packet in answer.split('\u{1e}') {
      if packet.starts_with('4') && packet.contains("/user") {
        return packet.chars().take(2).collect();
      }
      if packet == "2" {
        with_origin(http.post(&polling)).body("3").send().await.expect("the pong");
      }
    }
  }

  panic!("the server never answered the namespace connect");
}

#[test]
fn test_a_socket_from_another_sites_page_is_refused() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;

    assert_eq!(open_user_socket(&http, base, None).await, "40", "the account's own client connects");
    assert_eq!(
      open_user_socket(&http, base, Some("https://evil.example")).await,
      "44",
      "another site's page carrying the visitor's cookie is refused the connection"
    );
  });
}

#[test]
fn test_a_socket_is_held_to_the_account_as_it_stands() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;

    assert_eq!(open_user_socket(&http, base, None).await, "40");

    entity::users::ActiveModel {
      id: sea_orm::ActiveValue::Unchanged(user.model.id),
      is_enabled: Set(false),
      ..Default::default()
    }
    .update(&db)
    .await
    .expect("disable the account");

    assert_eq!(
      open_user_socket(&http, base, None).await,
      "44",
      "a cookie of an account disabled since is refused a socket, as it is refused every HTTP route"
    );
  });
}

// ─── cross-site requests ───────────────────────────────────────────────────────────────────────

#[test]
fn test_a_cookie_request_from_another_sites_page_is_refused() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let http = signed_in(base, &user).await;
    let own = format!("http://{base}");

    let rename = |page: Option<(reqwest::header::HeaderName, String)>| {
      let request = http
        .patch(at(base, "/me"))
        .json(&json!({ "fullName": "renamed", "email": user.model.email }));
      let request = match page {
        Some((header, value)) => request.header(header, value),
        None => request,
      };
      async move { outcome(request.send().await.expect("the /me request")).await }
    };

    assert_eq!(
      rename(Some((reqwest::header::ORIGIN, "https://evil.example".to_owned()))).await,
      (403, "E1019".to_owned()),
      "another site's page cannot change the account with the visitor's cookie"
    );
    assert_eq!(
      rename(Some((reqwest::header::REFERER, "https://evil.example/form.html".to_owned()))).await,
      (403, "E1019".to_owned()),
      "where the browser leaves out `Origin`, the `Referer` names the page"
    );
    assert_eq!(
      rename(Some((reqwest::header::ORIGIN, "null".to_owned()))).await,
      (403, "E1019".to_owned()),
      "an opaque origin is no page this server answers"
    );

    let reread = Users::find_by_id(user.model.id).one(&db).await.unwrap().unwrap();
    assert_ne!(reread.full_name.as_deref(), Some("renamed"), "none of the refused requests changed it");

    assert_eq!(
      rename(Some((reqwest::header::ORIGIN, own.clone()))).await.0,
      200,
      "the server's own page is answered"
    );
    assert_eq!(
      rename(Some((reqwest::header::REFERER, format!("{own}/settings")))).await.0,
      200,
      "so is its own page named by the referer"
    );
    assert_eq!(rename(None).await.0, 200, "a request that names no page is left to its cookie");

    assert_eq!(
      outcome(
        http
          .get(at(base, "/me"))
          .header(reqwest::header::ORIGIN, "https://evil.example")
          .send()
          .await
          .expect("the read"),
      )
      .await
      .0,
      200,
      "a read changes nothing, and is answered whichever page asks"
    );
  });
}

#[test]
fn test_a_header_credential_is_not_held_to_the_page_it_came_from() {
  runtime().block_on(async {
    let (_mock, base, db) = serving().await;
    let user = TestUser::create(&db).await.expect("an account");
    let key = api_key(&db, user.model.id, false).await;

    assert_eq!(
      outcome(
        caller()
          .patch(at(base, "/me"))
          .header(reqwest::header::AUTHORIZATION, format!("ApiKey {key}"))
          .header(reqwest::header::ORIGIN, "https://evil.example")
          .json(&json!({ "fullName": "renamed by key", "email": user.model.email }))
          .send()
          .await
          .expect("the /me request"),
      )
      .await
      .0,
      200,
      "another site's page cannot attach a visitor's credential header, so naming one is not refused"
    );
  });
}
