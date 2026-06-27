//! Background-maintenance scheduler jobs (`api::service::membership::maintenance`).
//!
//! The shared test DB is migrated fresh once per binary, so these jobs run over
//! rows left by other tests too — every assertion here targets the specific row
//! the test created, never a global count.

use api::service::membership::maintenance;
use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::membership::credit_transactions::Column as TxCol;
use entity::membership::subscription_events::Column as EventCol;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde_json::{Value, json};

/// A minimal `SumidaState` so the generic maintenance jobs can be invoked directly.
#[derive(Clone)]
struct MState {
  db: sea_orm::DatabaseConnection,
}
impl api::SumidaState for MState {
  fn db(&self) -> &sea_orm::DatabaseConnection {
    &self.db
  }
}

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

async fn admin_mock(db: &sea_orm::DatabaseConnection) -> Mock {
  let m = Mock::new().await;
  let admin = TestUser::create_admin(db).await.unwrap();
  admin.login(&m.client).await;
  m
}

/// Create a plan as admin; returns its `code`.
async fn create_plan(client: &TestClient, body: Value) -> String {
  let resp = client.post("/membership-plans").json(&body).send().await.expect("create plan");
  assert_eq!(resp.status(), StatusCode::CREATED, "admin should create plan");
  resp.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// A past-period active subscription transitions to `expired` (+ audit event).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_expires_due_subscription() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("mx-{}", rnd()), "name": "M", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;
  let created: Value = user_mock
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Force the period to have ended (status stays active until the job runs).
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = sub.into_active_model();
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(&db).await.unwrap();

  let n = maintenance::expire_due_subscriptions(&db).await.unwrap();
  assert!(n >= 1, "at least our subscription should be expired");

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    after.status,
    entity::membership::SubscriptionStatus::Expired,
    "due active subscription must become expired"
  );

  let events = SubscriptionEvents::find()
    .filter(EventCol::SubscriptionId.eq(sub_id))
    .filter(EventCol::NewStatus.eq("expired"))
    .all(&db)
    .await
    .unwrap();
  assert!(!events.is_empty(), "an expiry audit event must be recorded");
}

// ---------------------------------------------------------------------------
// cancel_at_period_end reaching the period end transitions to `canceled`.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_cancel_at_period_end_becomes_canceled() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("mc-{}", rnd()), "name": "M", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;
  let created: Value = user_mock
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Flagged to cancel at period end, and the period has now ended.
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = sub.into_active_model();
  a.cancel_at_period_end = Set(true);
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(&db).await.unwrap();

  maintenance::expire_due_subscriptions(&db).await.unwrap();

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    after.status,
    entity::membership::SubscriptionStatus::Canceled,
    "cancel-at-period-end reaching the end must become canceled"
  );
  assert!(after.canceled_at.is_some(), "canceled_at must be set");

  let events = SubscriptionEvents::find()
    .filter(EventCol::SubscriptionId.eq(sub_id))
    .filter(EventCol::NewStatus.eq("canceled"))
    .all(&db)
    .await
    .unwrap();
  assert!(!events.is_empty(), "a cancellation audit event must be recorded");
}

// ---------------------------------------------------------------------------
// An untouched wallet holding only expired lots is swept by the background job:
// balance drops to 0 and an `expire` ledger row is written.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_sweeps_inactive_wallet() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("ms-{}", rnd()), "name": "Cr", "kind": "credit", "scope": "personal",
      "actions": [{ "action": format!("act-{}", rnd()), "creditCost": 1 }],
    }),
  )
  .await;

  let user = TestUser::create(&db).await.unwrap();
  let past = (Utc::now() - Duration::hours(1)).to_rfc3339();

  // Grant an already-expired lot. The cached balance reflects it until swept,
  // and nothing touches the wallet afterwards.
  let grant: Value = am
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 40, "planCode": code, "expiryAt": past }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let wallet_id = grant["walletId"].as_i64().unwrap() as i32;

  // Cache still shows the (expired) 40 because grant sweeps before inserting.
  let before = CreditWallets::find_by_id(wallet_id).one(&db).await.unwrap().unwrap();
  assert_eq!(before.balance, 40, "cached balance includes the not-yet-swept expired lot");

  let swept = maintenance::sweep_expired_lots::<MState>(&db).await.unwrap();
  assert!(swept >= 1, "at least our wallet should be swept");

  let after = CreditWallets::find_by_id(wallet_id).one(&db).await.unwrap().unwrap();
  assert_eq!(after.balance, 0, "expired lot must be swept from the cached balance");

  let expires = CreditTransactions::find()
    .filter(TxCol::WalletId.eq(wallet_id))
    .filter(TxCol::Kind.eq("expire"))
    .all(&db)
    .await
    .unwrap();
  assert!(!expires.is_empty(), "an expire ledger row must be written");
}

// ---------------------------------------------------------------------------
// Expiring-soon notice fires once per period (dedup), and renewal re-arms it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_expiring_soon_dedup_and_renew_rearms() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let am = admin_mock(&db).await;
  // durationDays=3 puts current_period_end inside the default 7-day notice window.
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("me-{}", rnd()), "name": "M", "kind": "subscription", "scope": "personal",
      "durationDays": 3, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;
  let created: Value = user_mock
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // First pass marks and "sends".
  maintenance::notify_expiring_subscriptions::<MState>(&db).await.unwrap();
  let first = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let marked_at = first.expiring_notified_at.expect("expiring_notified_at must be set after notice");

  // Second pass must not re-notify this subscription (marker unchanged).
  maintenance::notify_expiring_subscriptions::<MState>(&db).await.unwrap();
  let second = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    second.expiring_notified_at,
    Some(marked_at),
    "already-notified period must not re-notify"
  );

  // Renewing re-arms the notice (marker cleared)…
  let r = user_mock.client.post(&format!("/subscriptions/{sub_id}/renew")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);
  let renewed = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(renewed.expiring_notified_at.is_none(), "renewal must clear the expiring marker");

  // …and the next pass notifies again (new end = ~6 days, still in window).
  maintenance::notify_expiring_subscriptions::<MState>(&db).await.unwrap();
  let again = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(again.expiring_notified_at.is_some(), "renewed subscription must notify again");
}

// ---------------------------------------------------------------------------
// Low-balance alert fires once (dedup) and re-arms on top-up above threshold.
// Requires `[membership] credit-low-balance-threshold` in settings.e2e.toml.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_low_balance_alert_dedup_and_topup_rearms() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let Some(threshold) = infra::settings::get().membership.credit_low_balance_threshold else {
    eprintln!("skipping: no membership.credit_low_balance_threshold configured");
    return;
  };

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("ml-{}", rnd()), "name": "Cr", "kind": "credit", "scope": "personal",
      "actions": [{ "action": format!("act-{}", rnd()), "creditCost": 1 }],
    }),
  )
  .await;

  // Grant exactly the threshold → balance == threshold → at/below threshold.
  let user = TestUser::create(&db).await.unwrap();
  let grant: Value = am
    .client
    .post("/credits/grant")
    .json(
      &json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": threshold, "planCode": code }),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let wallet_id = grant["walletId"].as_i64().unwrap() as i32;

  maintenance::notify_low_balance_wallets::<MState>(&db).await.unwrap();
  let first = CreditWallets::find_by_id(wallet_id).one(&db).await.unwrap().unwrap();
  let marked_at = first.low_balance_notified_at.expect("low_balance_notified_at must be set");

  // Dedup: a second pass does not re-alert (marker unchanged).
  maintenance::notify_low_balance_wallets::<MState>(&db).await.unwrap();
  let second = CreditWallets::find_by_id(wallet_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    second.low_balance_notified_at,
    Some(marked_at),
    "already-alerted wallet must not re-alert"
  );

  // Top up above the threshold → marker cleared by the grant.
  am.client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": threshold + 1, "planCode": code }))
    .send()
    .await
    .unwrap();
  let topped = CreditWallets::find_by_id(wallet_id).one(&db).await.unwrap().unwrap();
  assert!(topped.low_balance_notified_at.is_none(), "top-up above threshold must clear the marker");
  assert!(topped.balance > threshold, "balance must now exceed the threshold");
}
