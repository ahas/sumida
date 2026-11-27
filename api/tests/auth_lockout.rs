//! What a run of wrong passwords costs.
//!
//! The lockout is the one refusal that no single request can produce: it is decided from the log of
//! the attempts before it, and it turns a sign-in that was being refused for one reason into one
//! refused for another. Nothing else asks for it, and the policy it reads has defaults, so a
//! threshold that stopped being reached — or one that started being reached at the first attempt —
//! would look exactly like the suite passing.
//!
//! It sits in a binary of its own. The policy's default target is the address the attempts came
//! from, so the ban this case provokes catches every request from the same place while it stands,
//! including the ones another case would be making.

use e2e::prelude::*;
use entity::prelude::*;
use sea_orm::EntityTrait;
use serde_json::json;

/// The default policy this case drives to its threshold. Nothing in the e2e settings names a ban
/// policy, so these are the built-in defaults, and the case says which of them it depends on.
const ALLOWED_FAILURES: usize = 10;

/// Held for the whole of each case. Both cases sign in from the same address, so a run one of them
/// provokes bans the other's attempts, and the bans one of them clears are the other's too: run side
/// by side, each would read the other's lockout as its own.
static ONE_RUN_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn sign_in(http: &reqwest::Client, base: &str, username: &str, password: &str) -> (u16, String) {
  let resp = http
    .post(format!("{base}/auth/session"))
    .json(&json!({ "username": username, "password": password, "rememberMe": false }))
    .send()
    .await
    .expect("the sign-in request");

  let status = resp.status().as_u16();
  let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
  let code = body.get("code").and_then(|code| code.as_str()).unwrap_or_default().to_owned();

  (status, code)
}

#[tokio::test]
async fn test_a_run_of_wrong_passwords_locks_the_attempts_out() {
  let _alone = ONE_RUN_AT_A_TIME.lock().await;
  let mock = Mock::new().await;
  let base = format!("http://{}", mock.server.addr);
  let db = infra::db::connect().await;
  let user = TestUser::create(&db).await.expect("an account");

  let http = reqwest::Client::builder().cookie_store(true).build().expect("an http client");

  // Up to the threshold, each attempt is answered as the wrong password it is. The account is not
  // locked out on the way there, or the threshold would not be the threshold.
  for attempt in 1..ALLOWED_FAILURES {
    assert_eq!(
      sign_in(&http, &base, &user.model.username, "not-the-password").await,
      (401, "E1000".to_owned()),
      "attempt {attempt} of {ALLOWED_FAILURES} is a wrong password and nothing more"
    );
  }

  assert_eq!(
    sign_in(&http, &base, &user.model.username, "not-the-password").await,
    (401, "E1002".to_owned()),
    "the attempt that completes the run is refused by the lockout it provoked, not by the password"
  );

  assert_eq!(
    sign_in(&http, &base, &user.model.username, &user.password).await,
    (401, "E1002".to_owned()),
    "and the right password is refused too — a lockout that the correct password walks through is \
     no lockout"
  );

  // The disposition is on the address these attempts came from, so it stands between that address
  // and every account until it is cleared.
  Bans::delete_many().exec(&db).await.expect("clear the lockout");

  assert_eq!(
    sign_in(&http, &base, &user.model.username, &user.password).await.0,
    200,
    "with the lockout gone the same password gets in — the run is what refused it, and nothing else"
  );
}

#[tokio::test]
async fn test_a_password_set_anew_against_a_code_lifts_the_lockout_and_starts_the_count_again() {
  use chrono::{Duration, Utc};
  use sea_orm::{ActiveModelTrait, Set};

  let _alone = ONE_RUN_AT_A_TIME.lock().await;
  let mock = Mock::new().await;
  let base = format!("http://{}", mock.server.addr);
  let db = infra::db::connect().await;
  let user = TestUser::create(&db).await.expect("an account");
  let http = reqwest::Client::builder().cookie_store(true).build().expect("an http client");

  for _ in 0..ALLOWED_FAILURES {
    sign_in(&http, &base, &user.model.username, "not-the-password").await;
  }

  assert_eq!(
    sign_in(&http, &base, &user.model.username, &user.password).await,
    (401, "E1002".to_owned()),
    "the run has locked the attempts out"
  );

  // The owner of the account proves it with the code they were sent and sets a new password.
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

  let changed = http
    .post(format!("{base}/users/change-password"))
    .json(
      &json!({ "username": user.model.username, "password": "a-new-password-entirely", "code": "123456" }),
    )
    .send()
    .await
    .expect("the change-password request");

  assert_eq!(changed.status().as_u16(), 200, "the change is taken");

  // The lockout is lifted, and the failures before the change no longer count: one more wrong password is a
  // wrong password and nothing more, and the new password gets in.
  assert_eq!(
    sign_in(&http, &base, &user.model.username, "not-the-password").await,
    (401, "E1000".to_owned()),
    "a wrong password after the change starts a new run rather than completing the old one"
  );
  assert_eq!(sign_in(&http, &base, &user.model.username, "a-new-password-entirely").await.0, 200);

  Bans::delete_many().exec(&db).await.expect("leave no lockout behind");
}

#[tokio::test]
async fn test_guesses_spread_over_accounts_still_lock_the_address_out() {
  let _alone = ONE_RUN_AT_A_TIME.lock().await;
  let mock = Mock::new().await;
  let base = format!("http://{}", mock.server.addr);
  let db = infra::db::connect().await;
  Bans::delete_many().exec(&db).await.expect("start with no lockout");
  let first = TestUser::create(&db).await.expect("one account");
  let second = TestUser::create(&db).await.expect("another account");
  let http = reqwest::Client::builder().cookie_store(true).build().expect("an http client");

  // Fewer guesses than the limit at each account, but more than the limit from the one address. The
  // address is what the default policy bans, so it is the address whose guesses are counted.
  let half = ALLOWED_FAILURES / 2 + 1;
  for _ in 0..half {
    sign_in(&http, &base, &first.model.username, "not-the-password").await;
  }
  for _ in 0..half - 1 {
    sign_in(&http, &base, &second.model.username, "not-the-password").await;
  }

  assert_eq!(
    sign_in(&http, &base, &second.model.username, &second.password).await,
    (401, "E1002".to_owned()),
    "spreading the guesses over two accounts does not keep the address under the limit"
  );

  // Lifted the way the service lifts a ban: the row stays, marked lifted.
  Bans::update_many()
    .col_expr(entity::bans::Column::LiftedAt, sea_orm::sea_query::Expr::value(chrono::Utc::now()))
    .exec(&db)
    .await
    .expect("lift the lockout");
  assert_eq!(
    sign_in(&http, &base, &first.model.username, "not-the-password").await,
    (401, "E1000".to_owned()),
    "once the ban is lifted the guesses that earned it are not counted again"
  );
  assert_eq!(sign_in(&http, &base, &second.model.username, &second.password).await.0, 200);
}
