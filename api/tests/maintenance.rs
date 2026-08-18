//! Background-maintenance scheduler jobs (`api::service::membership::maintenance`).
//!
//! The shared test DB is migrated fresh once per binary, so these jobs run over
//! rows left by other tests too — every assertion here targets the specific row
//! the test created, never a global count.

use api::service::membership::{maintenance, org_manager_emails};
use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::ap_test_utils::grant_org_scope;
use e2e::prelude::*;
use entity::membership::credit_transactions::Column as TxCol;
use entity::membership::subscription_events::Column as EventCol;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{
  ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter, Set,
  Statement, TransactionTrait,
};
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
// run_once is guarded by an advisory lock: while another session holds it, a
// pass is skipped entirely (multi-instance single-runner guarantee).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_run_once_skips_when_lock_held() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("lk-{}", rnd()), "name": "LK", "kind": "subscription", "scope": "personal",
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
  // Make it due for expiry.
  let mut a = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap().into_active_model();
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(&db).await.unwrap();

  // Hold the maintenance advisory lock on a separate session.
  let lock_key: i64 = 0x5346_4D49_4D42_4C4B;
  let guard = db.begin().await.unwrap();
  guard
    .execute(Statement::from_sql_and_values(
      guard.get_database_backend(),
      "SELECT pg_advisory_xact_lock($1)",
      [lock_key.into()],
    ))
    .await
    .unwrap();

  // run_once must skip → our due subscription is NOT expired.
  maintenance::run_once::<MState>(&db).await;
  let still = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    still.status,
    entity::membership::SubscriptionStatus::Active,
    "while the lock is held, the tick is skipped"
  );

  // Release the lock → the next pass processes it.
  guard.commit().await.unwrap();
  maintenance::run_once::<MState>(&db).await;
  let done = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    done.status,
    entity::membership::SubscriptionStatus::Expired,
    "once unlocked, the pass expires the due subscription"
  );
}

// ---------------------------------------------------------------------------
// An abandoned (stale pending) checkout is expired and its coupon is reversed,
// freeing the per-owner cap. Requires mock PG + a configured TTL.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_stale_pending_purchase_reverses_coupon() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if infra::settings::get().membership.payment.provider != "mock"
    || infra::settings::get().membership.pending_purchase_ttl_hours <= 0
  {
    eprintln!("skipping: mock PG / pending_purchase_ttl_hours not configured");
    return;
  }
  let am = admin_mock(&db).await;
  let plan = create_plan(
    &am.client,
    json!({
      "code": format!("ab-{}", rnd()), "name": "AB", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;
  let code = format!("AB1-{}", rnd());
  am.client
    .post("/coupons")
    .json(&json!({ "code": code, "percentOff": 10, "maxRedemptionsPerOwner": 1 }))
    .send()
    .await
    .unwrap();

  let user_mock = Mock::new().await;
  TestUser::create(&db).await.unwrap().login(&user_mock.client).await;
  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });

  // First checkout records the redemption (per-owner cap 1); a second is blocked.
  let co: Value = user_mock
    .client
    .post("/payments/checkout")
    .json(&body)
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let pc = co["purchaseCode"].as_str().unwrap().to_string();
  assert_eq!(
    user_mock.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "per-owner cap reached while the first checkout is pending"
  );

  // Age the pending purchase past the TTL, then run the expiry job.
  let purchase = MembershipPurchases::find()
    .filter(entity::membership::membership_purchases::Column::Code.eq(pc))
    .one(&db)
    .await
    .unwrap()
    .unwrap();
  let mut a = purchase.into_active_model();
  a.created_at = Set((Utc::now() - Duration::hours(48)).into());
  a.update(&db).await.unwrap();

  let expired = maintenance::expire_stale_pending_purchases::<MState>(&db).await.unwrap();
  assert!(expired >= 1, "the stale pending purchase is expired");

  // The coupon was reversed → the owner can redeem again.
  assert_eq!(
    user_mock.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED,
    "abandoned checkout's coupon was returned, freeing the cap"
  );
}

// ---------------------------------------------------------------------------
// An organization's managers are resolved from its org policies for notices.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_org_manager_emails_resolution() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let org = TestOrg::create(&db).await.unwrap();
  let manager = TestUser::create_in_org(&db, org.model.id).await.unwrap();
  // 종전 `org:subscription | <org> | {manage}` 정책 행이 뜻하던 층.
  grant_org_scope(&db, &manager, org.model.id, "subscription_manager").await.unwrap();

  let emails = org_manager_emails(org.model.id, &db).await.unwrap();
  assert!(
    emails.iter().any(|e| Some(e.as_str()) == manager.model.email.as_deref()),
    "the org manager's email resolves from its `subscription_manager` grant"
  );
}

// ---------------------------------------------------------------------------
// Organization subscriptions are now processed by the expiring-soon job (the
// notice targets the org's managers, not just personal owners).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_org_subscription_expiring_processed() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  // durationDays=3 puts the first period inside the default 7-day window.
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("oe-{}", rnd()), "name": "OE", "kind": "subscription", "scope": "organization",
      "durationDays": 3, "actions": [{ "action": "a" }],
    }),
  )
  .await;
  let org = TestOrg::create(&db).await.unwrap();
  // The system admin can subscribe on behalf of the org.
  let created: Value = am
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "ownerKind": "organization", "ownerId": org.model.id }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  maintenance::notify_expiring_subscriptions::<MState>(&db).await.unwrap();
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(
    sub.expiring_notified_at.is_some(),
    "org subscriptions are processed by the expiring-soon job"
  );
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

/// Subscribe `client` as a logged-in user; returns the new subscription id.
async fn subscribe_user(db: &sea_orm::DatabaseConnection, body: Value) -> (Mock, i32) {
  let m = Mock::new().await;
  let user = TestUser::create(db).await.unwrap();
  user.login(&m.client).await;
  let created: Value =
    m.client.post("/subscriptions").json(&body).send().await.unwrap().json().await.unwrap();
  let id = created["id"].as_i64().expect("subscription id") as i32;
  (m, id)
}

// ---------------------------------------------------------------------------
// Auto-renew: a due subscription with a good billing key is charged and extended
// (requires the mock PG provider; otherwise self-skips).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_auto_renew_success() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("ar-{}", rnd()), "name": "AR", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  // Subscribe with auto-renew + a billing key the mock PG accepts.
  let (_m, sub_id) =
    subscribe_user(&db, json!({ "planCode": code, "autoRenew": true, "billingKey": "mock:bill:ok" })).await;

  // Force the period to have ended.
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = sub.into_active_model();
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(&db).await.unwrap();

  let (renewed, failed) = maintenance::renew_due_subscriptions(&db).await.unwrap();
  assert!(renewed >= 1, "our subscription should be renewed");
  assert_eq!(failed, 0, "a good billing key must not fail");

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(after.status, entity::membership::SubscriptionStatus::Active, "renewed → still active");
  assert!(after.current_period_end > Utc::now(), "renew must push the period into the future");

  let renewed_events = SubscriptionEvents::find()
    .filter(EventCol::SubscriptionId.eq(sub_id))
    .filter(EventCol::Reason.eq("auto-renew"))
    .all(&db)
    .await
    .unwrap();
  assert!(!renewed_events.is_empty(), "an auto-renew event must be recorded");
}

// ---------------------------------------------------------------------------
// Auto-renew: a declined billing key moves the subscription to past_due.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_maintenance_auto_renew_failure_past_due() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("af-{}", rnd()), "name": "AF", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  // A billing key the mock PG declines.
  let (_m, sub_id) =
    subscribe_user(&db, json!({ "planCode": code, "autoRenew": true, "billingKey": "mock:bill:nope" }))
      .await;

  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = sub.into_active_model();
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(&db).await.unwrap();

  let (renewed, _failed) = maintenance::renew_due_subscriptions(&db).await.unwrap();
  assert_eq!(renewed, 0, "a declined charge must not renew our subscription");

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    after.status,
    entity::membership::SubscriptionStatus::PastDue,
    "declined auto-renew → past_due"
  );

  let events = SubscriptionEvents::find()
    .filter(EventCol::SubscriptionId.eq(sub_id))
    .filter(EventCol::Reason.eq("auto-renew-charge-failed"))
    .all(&db)
    .await
    .unwrap();
  assert!(!events.is_empty(), "a charge-failed status_changed event must be recorded");
}

// ---------------------------------------------------------------------------
// Enabling auto-renew requires a billing key; subscribe + endpoint both enforce it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_auto_renew_requires_billing_key() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("ab-{}", rnd()), "name": "AB", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  // Subscribe with auto-renew but no billing key → 400.
  let m = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&m.client).await;
  let resp = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "autoRenew": true }))
    .send()
    .await
    .unwrap();
  assert_eq!(
    resp.status(),
    StatusCode::BAD_REQUEST,
    "auto_renew without billing_key must be rejected"
  );

  // Subscribe normally, then toggle auto-renew via the endpoint.
  let created: Value = m
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

  // Enabling without a stored/supplied key → 400.
  let resp = m
    .client
    .post(&format!("/subscriptions/{sub_id}/auto-renew"))
    .json(&json!({ "enabled": true }))
    .send()
    .await
    .unwrap();
  assert_eq!(
    resp.status(),
    StatusCode::BAD_REQUEST,
    "enabling without a billing key must be rejected"
  );

  // Enabling with a key → 200, and it sticks.
  let resp = m
    .client
    .post(&format!("/subscriptions/{sub_id}/auto-renew"))
    .json(&json!({ "enabled": true, "billingKey": "mock:bill:ok" }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK, "enabling with a billing key must succeed");

  let got: Value = m
    .client
    .get(&format!("/subscriptions/{sub_id}"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(got["autoRenew"].as_bool().unwrap(), true, "auto-renew must be enabled");
}
