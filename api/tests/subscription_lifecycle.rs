//! Subscription lifecycle extras: reactivation, trial/grace, dunning.
//! Each assertion targets the subscription the test created (shared test DB).

use api::service::membership::maintenance;
use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::membership::membership_purchases::Column as PurchaseCol;
use entity::membership::subscription_events::Column as EventCol;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde_json::{Value, json};

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

async fn admin_mock(db: &sea_orm::DatabaseConnection) -> Mock {
  let m = Mock::new().await;
  let admin = TestUser::create_admin(db).await.unwrap();
  admin.login(&m.client).await;
  m
}

async fn create_plan(client: &TestClient, body: Value) -> String {
  let resp = client.post("/membership-plans").json(&body).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  resp.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string()
}

async fn user_mock(db: &sea_orm::DatabaseConnection) -> (Mock, TestUser) {
  let m = Mock::new().await;
  let u = TestUser::create(db).await.unwrap();
  u.login(&m.client).await;
  (m, u)
}

// ---------------------------------------------------------------------------
// A canceled subscription can be reactivated into a fresh active period.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reactivate_canceled_subscription() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("re-{}", rnd()), "name": "RE", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
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

  // Cancel immediately → canceled, entitlement stops.
  assert_eq!(
    m.client.post(&format!("/subscriptions/{sub_id}/cancel")).send().await.unwrap().status(),
    StatusCode::OK
  );
  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN);

  // Reactivate → active again, entitlement restored.
  let r = m.client.post(&format!("/subscriptions/{sub_id}/reactivate")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK, "canceled subscription must reactivate");

  let got: Value = m
    .client
    .get(&format!("/subscriptions/{sub_id}"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(got["status"].as_str().unwrap(), "active");
  assert!(got["canceledAt"].is_null(), "reactivation clears canceled_at");

  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK, "reactivated subscription must entitle again");

  let events = SubscriptionEvents::find()
    .filter(EventCol::SubscriptionId.eq(sub_id))
    .filter(EventCol::EventKind.eq("reactivated"))
    .all(&db)
    .await
    .unwrap();
  assert!(!events.is_empty(), "a reactivated event must be recorded");
}

// ---------------------------------------------------------------------------
// Reactivating an already-active subscription is rejected.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reactivate_active_rejected() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("ra-{}", rnd()), "name": "RA", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
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

  let r = m.client.post(&format!("/subscriptions/{sub_id}/reactivate")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "active subscription must not reactivate");
}

// ---------------------------------------------------------------------------
// A plan with trial_days starts a free first period (amount 0, trial_ends_at set).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_trial_period_is_free_and_short() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("tr-{}", rnd()), "name": "TR", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "trialDays": 7, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
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

  // Trial: entitled now, first period ≈ 7 days (not 30), trial_ends_at set.
  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK, "trial subscription is entitled");

  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(sub.trial_ends_at.is_some(), "trial_ends_at must be set");
  assert!(
    sub.current_period_end < Utc::now() + Duration::days(8),
    "trial first period is ~7 days, not 30"
  );

  // The trial purchase recorded amount 0 (no charge taken).
  let purchase = MembershipPurchases::find()
    .filter(PurchaseCol::OwnerId.eq(_u.model.id))
    .filter(PurchaseCol::PlanCode.eq(code))
    .one(&db)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(purchase.amount, 0, "no charge during trial");
}

// ---------------------------------------------------------------------------
// After a failed renewal, entitlement continues through the grace window, then
// stops once grace ends. Requires `[membership] grace-days` > 0 (else self-skips).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_grace_window_keeps_entitlement_then_expires() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if infra::settings::get().membership.grace_days <= 0 {
    eprintln!("skipping: membership.grace_days not configured (> 0)");
    return;
  }
  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }

  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("gr-{}", rnd()), "name": "GR", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  // Auto-renew with a billing key the mock PG declines.
  let (m, _u) = user_mock(&db).await;
  let created: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "autoRenew": true, "billingKey": "mock:bill:fail" }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Force the period to end, then run the renewal job → declined → past_due + grace.
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = sub.into_active_model();
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(&db).await.unwrap();

  let (_renewed, failed) = maintenance::renew_due_subscriptions(&db).await.unwrap();
  assert!(failed >= 1, "the declined renewal should fail");

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(after.status, entity::membership::SubscriptionStatus::PastDue);
  assert!(after.grace_ends_at.is_some(), "a grace window must open");

  // Still entitled during grace.
  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK, "entitlement continues during grace");

  // Push grace into the past → entitlement stops.
  let row = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = row.into_active_model();
  a.grace_ends_at = Set(Some((Utc::now() - Duration::hours(1)).into()));
  a.update(&db).await.unwrap();

  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "entitlement stops once grace ends");
}

/// Returns true when grace + dunning + mock PG are all configured for the run.
fn dunning_ready() -> bool {
  let s = infra::settings::get();
  s.membership.grace_days > 0
    && s.membership.max_dunning_attempts > 0
    && s.membership.payment.provider == "mock"
}

/// Drive a fresh auto-renew subscription into past_due + grace via a declined
/// renewal. Returns `(user client, subscription id)`.
async fn past_due_with_grace(db: &sea_orm::DatabaseConnection, am: &Mock) -> (Mock, i32) {
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("dn-{}", rnd()), "name": "DN", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;
  let (m, _u) = user_mock(db).await;
  let created: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "autoRenew": true, "billingKey": "mock:bill:fail" }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  let sub = Subscriptions::find_by_id(sub_id).one(db).await.unwrap().unwrap();
  let mut a = sub.into_active_model();
  a.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  a.update(db).await.unwrap();
  maintenance::renew_due_subscriptions(db).await.unwrap();
  (m, sub_id)
}

// ---------------------------------------------------------------------------
// Dunning: a retry with a now-good billing key recovers the subscription.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_dunning_recovers_subscription() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if !dunning_ready() {
    eprintln!("skipping: grace/dunning/mock not configured");
    return;
  }
  let am = admin_mock(&db).await;
  let (m, sub_id) = past_due_with_grace(&db, &am).await;

  // The customer fixes their card → swap to an accepted billing key.
  let row = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(row.status, entity::membership::SubscriptionStatus::PastDue);
  let mut a = row.into_active_model();
  a.billing_key = Set(Some("mock:bill:ok".to_string()));
  a.update(&db).await.unwrap();

  let (recovered, _failed) = maintenance::retry_dunning_subscriptions(&db).await.unwrap();
  assert!(recovered >= 1, "dunning should recover with a good billing key");

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(after.status, entity::membership::SubscriptionStatus::Active, "recovered → active");
  assert!(after.grace_ends_at.is_none(), "grace cleared on recovery");
  assert_eq!(after.dunning_attempts, 0, "attempts reset on recovery");

  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK, "recovered subscription entitles again");
}

// ---------------------------------------------------------------------------
// Dunning caps retries at max_dunning_attempts (one retry per tick).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_dunning_caps_attempts() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if !dunning_ready() {
    eprintln!("skipping: grace/dunning/mock not configured");
    return;
  }
  let max = infra::settings::get().membership.max_dunning_attempts;
  let am = admin_mock(&db).await;
  let (_m, sub_id) = past_due_with_grace(&db, &am).await;

  // Each tick retries once (still the failing key) and increments attempts.
  for _ in 0..max {
    maintenance::retry_dunning_subscriptions(&db).await.unwrap();
  }
  let capped = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(capped.dunning_attempts, max, "attempts climb to the cap");
  assert_eq!(capped.status, entity::membership::SubscriptionStatus::PastDue, "still past_due");

  // A further tick is a no-op (attempts already at the cap).
  let (recovered, failed) = maintenance::retry_dunning_subscriptions(&db).await.unwrap();
  let still = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(still.dunning_attempts, max, "no retries past the cap");
  // (recovered, failed) here pertain to all eligible subs; ours contributes nothing.
  let _ = (recovered, failed);
}

// ---------------------------------------------------------------------------
// Once grace ends, the expire job finalizes a past_due subscription as expired.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_expire_finalizes_past_due_after_grace() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if !dunning_ready() {
    eprintln!("skipping: grace/dunning/mock not configured");
    return;
  }
  let am = admin_mock(&db).await;
  let (_m, sub_id) = past_due_with_grace(&db, &am).await;

  // Push grace into the past, then run the expiry job.
  let row = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = row.into_active_model();
  a.grace_ends_at = Set(Some((Utc::now() - Duration::hours(1)).into()));
  a.update(&db).await.unwrap();

  maintenance::expire_due_subscriptions(&db).await.unwrap();

  let after = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(after.status, entity::membership::SubscriptionStatus::Expired, "grace ended → expired");

  let events = SubscriptionEvents::find()
    .filter(EventCol::SubscriptionId.eq(sub_id))
    .filter(EventCol::Reason.eq("grace-ended"))
    .all(&db)
    .await
    .unwrap();
  assert!(!events.is_empty(), "a grace-ended expiry event must be recorded");
}

// ---------------------------------------------------------------------------
// Mid-period plan change (upgrade) prorates and switches coverage in place.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_change_plan_prorates_and_switches_coverage() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;

  let cheap = create_plan(
    &am.client,
    json!({
      "code": format!("cp-{}", rnd()), "name": "Cheap", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "old-act" }],
    }),
  )
  .await;
  let pricey = create_plan(
    &am.client,
    json!({
      "code": format!("pp-{}", rnd()), "name": "Pricey", "kind": "subscription", "scope": "personal",
      "price": 3000, "durationDays": 30, "actions": [{ "action": "new-act" }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
  let created: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": cheap }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Upgrade immediately (≈ full period remaining): net ≈ 3000 - 1000 = 2000.
  let res: Value = m
    .client
    .post(&format!("/subscriptions/{sub_id}/change-plan"))
    .json(&json!({ "planCode": pricey }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let proration = res["prorationAmount"].as_i64().unwrap();
  assert!((1980..=2000).contains(&proration), "upgrade proration ≈ 2000, got {proration}");
  assert_eq!(res["newPlanCode"].as_str().unwrap(), pricey);
  assert_eq!(res["oldPlanCode"].as_str().unwrap(), cheap);

  // The subscription is now on the new plan (period preserved, prev_plan_id set).
  let got: Value = m
    .client
    .get(&format!("/subscriptions/{sub_id}"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(got["status"].as_str().unwrap(), "active");
  assert_eq!(got["planCode"].as_str().unwrap(), pricey);
  let row = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(row.prev_plan_id.is_some(), "prev_plan_id records the prior plan");

  // Coverage switched: the new action is entitled, the old one is not.
  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "new-act" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK, "new plan action is entitled");
  let r = m
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "old-act" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "old plan action no longer covered");
}

// ---------------------------------------------------------------------------
// Changing to the same plan is rejected.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_change_plan_same_plan_rejected() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("sp-{}", rnd()), "name": "SP", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
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

  let r = m
    .client
    .post(&format!("/subscriptions/{sub_id}/change-plan"))
    .json(&json!({ "planCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "changing to the same plan must be rejected");
}

// ---------------------------------------------------------------------------
// An injected billing key is verified with the PG before it is stored.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_billing_key_verified_on_inject() {
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
      "code": format!("bk-{}", rnd()), "name": "BK", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;
  let (m, _u) = user_mock(&db).await;

  // An unrecognized billing key is rejected up front.
  let bad = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "autoRenew": true, "billingKey": "not-a-real-key" }))
    .send()
    .await
    .unwrap();
  assert_eq!(bad.status(), StatusCode::BAD_REQUEST, "invalid billing key is rejected");

  // A valid (issued) billing key is accepted.
  let ok = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "autoRenew": true, "billingKey": "mock:bill:ok" }))
    .send()
    .await
    .unwrap();
  assert_eq!(ok.status(), StatusCode::CREATED, "verified billing key is accepted");
  let sub_id = ok.json::<Value>().await.unwrap()["id"].as_i64().unwrap() as i32;

  // set_auto_renew also verifies an injected key.
  let r = m
    .client
    .post(&format!("/subscriptions/{sub_id}/auto-renew"))
    .json(&json!({ "enabled": true, "billingKey": "garbage" }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "set_auto_renew verifies the key too");
}

// ---------------------------------------------------------------------------
// A free trial is granted only once per (owner, plan); a re-subscribe is paid.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_trial_granted_only_once() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("t1-{}", rnd()), "name": "T1", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "trialDays": 7, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
  // First subscribe → trial.
  let first: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let first_id = first["id"].as_i64().unwrap() as i32;
  assert!(
    Subscriptions::find_by_id(first_id)
      .one(&db)
      .await
      .unwrap()
      .unwrap()
      .trial_ends_at
      .is_some()
  );

  // Cancel it, then re-subscribe to the same plan → no second trial (paid period).
  assert_eq!(
    m.client
      .post(&format!("/subscriptions/{first_id}/cancel"))
      .send()
      .await
      .unwrap()
      .status(),
    StatusCode::OK
  );
  let second: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let second_id = second["id"].as_i64().unwrap() as i32;
  let s = Subscriptions::find_by_id(second_id).one(&db).await.unwrap().unwrap();
  assert!(s.trial_ends_at.is_none(), "no repeat trial for the same plan");
  assert!(
    s.current_period_end > Utc::now() + Duration::days(20),
    "second period is the full ~30 days"
  );
}

// ---------------------------------------------------------------------------
// An upgrade settles its positive proration through the stored billing key.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_change_plan_settles_via_billing_key() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }
  let am = admin_mock(&db).await;
  let cheap = create_plan(
    &am.client,
    json!({ "code": format!("ps-{}", rnd()), "name": "C", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }] }),
  )
  .await;
  let pricey = create_plan(
    &am.client,
    json!({ "code": format!("pp-{}", rnd()), "name": "P", "kind": "subscription", "scope": "personal",
      "price": 3000, "durationDays": 30, "actions": [{ "action": "b" }] }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
  let created: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": cheap, "autoRenew": true, "billingKey": "mock:bill:ok" }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  let res: Value = m
    .client
    .post(&format!("/subscriptions/{sub_id}/change-plan"))
    .json(&json!({ "planCode": pricey }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let purchase_code = res["purchaseCode"].as_str().unwrap().to_string();

  // The proration purchase was charged at the mock PG.
  let purchase = MembershipPurchases::find()
    .filter(PurchaseCol::Code.eq(purchase_code))
    .one(&db)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(purchase.provider, "mock", "proration settled via the billing key");
  assert!(purchase.amount > 0);
}

// ---------------------------------------------------------------------------
// A declined proration charge aborts the change (plan unchanged).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_change_plan_charge_decline_aborts() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }
  let am = admin_mock(&db).await;
  let cheap = create_plan(
    &am.client,
    json!({ "code": format!("pf-{}", rnd()), "name": "C", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }] }),
  )
  .await;
  let pricey = create_plan(
    &am.client,
    json!({ "code": format!("pg-{}", rnd()), "name": "P", "kind": "subscription", "scope": "personal",
      "price": 3000, "durationDays": 30, "actions": [{ "action": "b" }] }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
  let created: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": cheap, "autoRenew": true, "billingKey": "mock:bill:fail" }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  let r = m
    .client
    .post(&format!("/subscriptions/{sub_id}/change-plan"))
    .json(&json!({ "planCode": pricey }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "declined proration charge → 402");

  // Plan unchanged.
  let still = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert_eq!(still.plan_code, cheap, "a declined charge must not switch the plan");
}
