//! Which attempts a ban catches.
//!
//! A ban row states a set of conditions and catches a request only when they **all** hold. Every
//! environment column defaults to the wildcard, so a row naming only an account leaves ip, user
//! agent, os and device open. Read as a disjunction, one open column is enough to catch every
//! request — a ban on one account locks out everybody, and the only symptom is that nobody can sign
//! in. Nothing else here fails, so only a case that asks the question catches it.
//!
//! Every case is serialized: the predicate is asked of the whole table, so a row another case left
//! behind is a row this one would count.

use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::prelude::*;
use infra::session::{is_banned, UserEnvironment};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

async fn db() -> sea_orm::DatabaseConnection {
  bootstrap::init_once().await;
  infra::db::connect().await
}

/// The environment a request arrives with. Two of them differ in exactly one column, so a case can
/// say which column it is testing.
fn arriving_from(ip: &str) -> UserEnvironment {
  UserEnvironment {
    ip_address: ip.to_owned(),
    user_agent: "e2e/1.0".to_owned(),
    os_family: "Linux".to_owned(),
    device_family: "Other".to_owned(),
    ..Default::default()
  }
}

/// Nothing from an earlier case may be live while this one asks the question.
async fn only_this_ban(db: &sea_orm::DatabaseConnection, ban: entity::bans::ActiveModel) -> i32 {
  Bans::delete_many().exec(db).await.expect("clear bans");

  ban.insert(db).await.expect("place the ban").id
}

async fn clear(db: &sea_orm::DatabaseConnection) {
  Bans::delete_many().exec(db).await.expect("clear bans");
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_an_account_ban_catches_that_account() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");

  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      ..Default::default()
    },
  )
  .await;

  assert!(
    is_banned(Some(target.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "the account the ban names is caught by it"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_an_account_ban_does_not_catch_another_account() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");
  let bystander = TestUser::create(&db).await.expect("an unrelated account");

  // The row names an account and leaves every environment column at its default. Those defaults are
  // the wildcard, and a wildcard means "any" — so unless the columns are read together, this row
  // catches the whole world.
  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      ..Default::default()
    },
  )
  .await;

  assert!(
    !is_banned(Some(bystander.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "an account nobody banned can still sign in while somebody else is banned"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_an_account_ban_does_not_catch_an_attempt_with_no_account() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");

  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      ..Default::default()
    },
  )
  .await;

  // A username that resolved to nobody has no account to compare against, and an account ban is
  // about an account.
  assert!(
    !is_banned(None, &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "an attempt that never resolved to an account is not caught by an account ban"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_an_environment_ban_catches_whoever_arrives_from_there() {
  let db = db().await;
  let whoever = TestUser::create(&db).await.expect("any account");

  // No account named. This is the kind of ban that is about where the request came from.
  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      ip_address: Set(Some("10.0.0.9".into())),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      ..Default::default()
    },
  )
  .await;

  assert!(
    is_banned(Some(whoever.model.id), &arriving_from("10.0.0.9"), &db).await.expect("ask"),
    "an environment ban catches an account it never names"
  );
  assert!(
    is_banned(None, &arriving_from("10.0.0.9"), &db).await.expect("ask"),
    "and it catches an attempt that resolved to no account at all"
  );
  assert!(
    !is_banned(Some(whoever.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "the same account arriving from elsewhere is not caught"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_the_columns_are_read_together() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");

  // Both an account and an address. Naming two things narrows the ban; it must not widen it.
  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      ip_address: Set(Some("10.0.0.9".into())),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      ..Default::default()
    },
  )
  .await;

  assert!(
    is_banned(Some(target.model.id), &arriving_from("10.0.0.9"), &db).await.expect("ask"),
    "the account, from the address the ban names"
  );
  assert!(
    !is_banned(Some(target.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "the same account from anywhere else is outside a ban that named an address"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_lifted_ban_catches_nobody() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");

  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      lifted_at: Set(Some(Utc::now())),
      ..Default::default()
    },
  )
  .await;

  assert!(
    !is_banned(Some(target.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "lifting is what undoes a ban, and the row stays for the record"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_term_that_has_run_out_catches_nobody() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");

  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now() - Duration::hours(2)),
      expires_at: Set(Some(Utc::now() - Duration::hours(1))),
      ..Default::default()
    },
  )
  .await;

  assert!(
    !is_banned(Some(target.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "a term that has passed no longer holds"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_term_still_running_holds() {
  let db = db().await;
  let target = TestUser::create(&db).await.expect("the banned account");

  only_this_ban(
    &db,
    entity::bans::ActiveModel {
      user_id: Set(Some(target.model.id)),
      reason: Set(Some("banned".into())),
      created_at: Set(Utc::now()),
      expires_at: Set(Some(Utc::now() + Duration::hours(1))),
      ..Default::default()
    },
  )
  .await;

  assert!(
    is_banned(Some(target.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "a term that has not passed still holds"
  );

  clear(&db).await;
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_no_ban_catches_nobody() {
  let db = db().await;
  let whoever = TestUser::create(&db).await.expect("any account");

  clear(&db).await;

  assert!(
    !is_banned(Some(whoever.model.id), &arriving_from("10.0.0.1"), &db).await.expect("ask"),
    "an empty table is not a ban"
  );
}
