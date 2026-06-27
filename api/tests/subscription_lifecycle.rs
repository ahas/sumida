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
  let created: Value =
    m.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap().json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Cancel immediately → canceled, entitlement stops.
  assert_eq!(m.client.post(&format!("/subscriptions/{sub_id}/cancel")).send().await.unwrap().status(), StatusCode::OK);
  let r = m.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN);

  // Reactivate → active again, entitlement restored.
  let r = m.client.post(&format!("/subscriptions/{sub_id}/reactivate")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK, "canceled subscription must reactivate");

  let got: Value =
    m.client.get(&format!("/subscriptions/{sub_id}")).send().await.unwrap().json().await.unwrap();
  assert_eq!(got["status"].as_str().unwrap(), "active");
  assert!(got["canceledAt"].is_null(), "reactivation clears canceled_at");

  let r = m.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
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
  let created: Value =
    m.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap().json().await.unwrap();
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
  let created: Value =
    m.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap().json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Trial: entitled now, first period ≈ 7 days (not 30), trial_ends_at set.
  let r = m.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK, "trial subscription is entitled");

  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(sub.trial_ends_at.is_some(), "trial_ends_at must be set");
  assert!(sub.current_period_end < Utc::now() + Duration::days(8), "trial first period is ~7 days, not 30");

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
  let r = m.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK, "entitlement continues during grace");

  // Push grace into the past → entitlement stops.
  let row = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut a = row.into_active_model();
  a.grace_ends_at = Set(Some((Utc::now() - Duration::hours(1)).into()));
  a.update(&db).await.unwrap();

  let r = m.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "entitlement stops once grace ends");
}
