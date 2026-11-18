//! Subscription quota metering (`plan_actions.quota_per_period` and
//! `plan_actions.amount_quota_per_period`).
//!
//! The entitlement subscription path counts uses and amounts per period and falls
//! through (to credits, or denial) once the quota is spent; renewal resets it. An
//! amount quota is also held, extended, captured and released.

use api::service::membership::maintenance;
use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{
  ActiveModelTrait,
  ColumnTrait,
  EntityTrait,
  IntoActiveModel,
  PaginatorTrait,
  QueryFilter,
  Set,
};
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

async fn check(client: &TestClient, action: &str) -> reqwest::Response {
  client
    .post("/entitlements/check")
    .json(&json!({ "action": action }))
    .send()
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// A quota-limited action passes up to the limit, then is denied (no credits).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_quota_limits_then_denies() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let action = format!("q-{}", rnd());
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("ql-{}", rnd()), "name": "QL", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": &action, "quotaPerPeriod": 2 }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
  assert_eq!(
    m.client
      .post("/subscriptions")
      .json(&json!({ "planCode": code }))
      .send()
      .await
      .unwrap()
      .status(),
    StatusCode::CREATED
  );

  // Two uses within quota go via the subscription.
  for _ in 0..2 {
    let r = check(&m.client, &action).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.json::<Value>().await.unwrap()["via"].as_str().unwrap(), "subscription");
  }

  // Third use: quota spent, no credit plan covers it → denied.
  let r = check(&m.client, &action).await;
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "quota exhausted and no credit fallback → denied");
}

// ---------------------------------------------------------------------------
// Once the subscription quota is spent, the gate falls through to credits.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_quota_falls_through_to_credit() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let action = format!("q-{}", rnd());

  // Subscription plan: 1 use/period. Credit plan: same action billable.
  let sub_code = create_plan(
    &am.client,
    json!({
      "code": format!("qf-{}", rnd()), "name": "QF", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": &action, "quotaPerPeriod": 1 }],
    }),
  )
  .await;
  let credit_code = create_plan(
    &am.client,
    json!({
      "code": format!("qc-{}", rnd()), "name": "QC", "kind": "credit", "scope": "personal",
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  let (m, u) = user_mock(&db).await;
  assert_eq!(
    m.client
      .post("/subscriptions")
      .json(&json!({ "planCode": sub_code }))
      .send()
      .await
      .unwrap()
      .status(),
    StatusCode::CREATED
  );
  am.client
    .post("/credits/grant")
    .json(
      &json!({ "ownerKind": "personal", "ownerId": u.model.id, "amount": 100, "planCode": credit_code }),
    )
    .send()
    .await
    .unwrap();

  // First use: subscription. Second use: quota spent → credit consumed.
  let r1 = check(&m.client, &action).await;
  assert_eq!(r1.json::<Value>().await.unwrap()["via"].as_str().unwrap(), "subscription");

  let r2 = check(&m.client, &action).await;
  let out2: Value = r2.json().await.unwrap();
  assert_eq!(out2["via"].as_str().unwrap(), "credit", "exhausted quota must fall through to credits");
  assert_eq!(out2["consumed"].as_i64().unwrap(), 10);
}

// ---------------------------------------------------------------------------
// An action without a quota (null) is unlimited within the subscription.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_unlimited_action_not_metered() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let action = format!("u-{}", rnd());
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("un-{}", rnd()), "name": "UN", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": &action }],
    }),
  )
  .await;

  let (m, _u) = user_mock(&db).await;
  m.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap();

  // Many uses all pass via the subscription (no quota).
  for _ in 0..5 {
    let r = check(&m.client, &action).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.json::<Value>().await.unwrap()["via"].as_str().unwrap(), "subscription");
  }
}

// ---------------------------------------------------------------------------
// Renewal starts a new period, resetting the quota.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_quota_resets_on_renewal() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let action = format!("q-{}", rnd());
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("qr-{}", rnd()), "name": "QR", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": &action, "quotaPerPeriod": 1 }],
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

  // Spend the single allowed use.
  assert_eq!(check(&m.client, &action).await.status(), StatusCode::OK);
  assert_eq!(check(&m.client, &action).await.status(), StatusCode::FORBIDDEN, "quota spent");

  // Renew → new period → quota available again.
  let r = m.client.post(&format!("/subscriptions/{sub_id}/renew")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  let r = check(&m.client, &action).await;
  assert_eq!(r.status(), StatusCode::OK, "renewal must reset the per-period quota");
  assert_eq!(r.json::<Value>().await.unwrap()["via"].as_str().unwrap(), "subscription");
}

async fn post(client: &TestClient, path: &str, body: Value) -> reqwest::Response {
  client.post(path).json(&body).send().await.unwrap()
}

/// A fresh user subscribed to a plan whose `action` may spend `limit` per period.
async fn amount_quota_user(
  db: &sea_orm::DatabaseConnection,
  am: &Mock,
  limit: i64,
) -> (Mock, TestUser, String) {
  let action = format!("aq-{}", rnd());
  let code = create_plan(
    &am.client,
    json!({
      "code": format!("aq-{}", rnd()), "name": "AQ", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": &action, "amountQuotaPerPeriod": limit }],
    }),
  )
  .await;
  let (m, u) = user_mock(db).await;
  assert_eq!(
    post(&m.client, "/subscriptions", json!({ "planCode": code })).await.status(),
    StatusCode::CREATED
  );
  (m, u, action)
}

// ---------------------------------------------------------------------------
// An amount quota is held, extended, captured at the actual amount and released,
// each step idempotent, with overage bounded by the caller's limit and allowed
// only by an administrator on a personal subscription.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_amount_quota_hold_steps() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let (m, _user, action) = amount_quota_user(&db, &am, 100).await;

  let reserve = json!({ "action": &action, "amount": 60, "idempotencyKey": "q-1" });
  let first: Value = post(&m.client, "/entitlements/reserve", reserve.clone()).await.json().await.unwrap();
  let again: Value = post(&m.client, "/entitlements/reserve", reserve).await.json().await.unwrap();
  assert_eq!(first["reservationId"], again["reservationId"], "a repeated reserve is the same hold");
  assert_eq!(again["remaining"].as_i64().unwrap(), 40);
  let id = first["reservationId"].as_i64().unwrap();

  let extend = format!("/entitlements/reservations/{id}/extend");
  for _ in 0..2 {
    let r: Value = post(&m.client, &extend, json!({ "amount": 30, "idempotencyKey": "q-2" }))
      .await
      .json()
      .await
      .unwrap();
    assert_eq!(r["amount"].as_i64().unwrap(), 90, "a repeated extension grows the hold once");
  }
  let r = post(&m.client, &extend, json!({ "amount": 20 })).await;
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "an extension past the room is refused");

  for _ in 0..2 {
    let r: Value = post(
      &m.client,
      &format!("/entitlements/reservations/{id}/capture"),
      json!({ "amount": 95, "idempotencyKey": "q-3" }),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
      r["amountUsed"].as_i64().unwrap(),
      95,
      "a capture over the hold records the actual amount, once"
    );
    assert_eq!(r["remaining"].as_i64().unwrap(), 5);
  }

  let r = post(&m.client, "/entitlements/reserve", json!({ "action": &action, "amount": 10 })).await;
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "a hold past the room is refused");

  let hold: Value = post(&m.client, "/entitlements/reserve", json!({ "action": &action, "amount": 5 }))
    .await
    .json()
    .await
    .unwrap();
  let capture = format!("/entitlements/reservations/{}/capture", hold["reservationId"].as_i64().unwrap());
  let r = post(&m.client, &capture, json!({ "amount": 20 })).await;
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "overage without a limit is refused");
  let r = post(&m.client, &capture, json!({ "amount": 20, "overageLimit": 50 })).await;
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "the owner may not allow their own overage");
  let r: Value = post(&am.client, &capture, json!({ "amount": 20, "overageLimit": 50 }))
    .await
    .json()
    .await
    .unwrap();
  assert_eq!(r["amountUsed"].as_i64().unwrap(), 115, "the overage is recorded in the period's usage");
  assert_eq!(r["remaining"].as_i64().unwrap(), 0);

  let r = post(&m.client, "/entitlements/reserve", json!({ "action": &action, "amount": 1 })).await;
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "a period past its limit has no room");
}

// ---------------------------------------------------------------------------
// The entitlement gate spends an amount from the quota while it has room, then
// from the wallet; a repeated key answers as the first call did.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_amount_check_uses_quota_then_credit() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let (m, u, action) = amount_quota_user(&db, &am, 50).await;
  post(&am.client, "/credits/grant", json!({ "ownerId": u.model.id, "amount": 100 })).await;

  let spend = |key: &'static str| json!({ "action": &action, "amount": 30, "idempotencyKey": key });
  let r: Value = post(&m.client, "/entitlements/check", spend("c-1")).await.json().await.unwrap();
  assert_eq!(r["via"].as_str().unwrap(), "subscription");
  let r: Value = post(&m.client, "/entitlements/check", spend("c-1")).await.json().await.unwrap();
  assert_eq!(r["via"].as_str().unwrap(), "subscription", "a repeated key is not spent again");

  let r: Value = post(&m.client, "/entitlements/check", spend("c-2")).await.json().await.unwrap();
  assert_eq!(
    r["via"].as_str().unwrap(),
    "credit",
    "20 left in the quota, so 30 comes from the wallet"
  );
  assert_eq!(r["consumed"].as_i64().unwrap(), 30);
}

// ---------------------------------------------------------------------------
// A quota hold past its expiry stops counting at once and cannot be spent; the
// scheduler releases it and records the release.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_expired_quota_hold_released() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let (m, _u, action) = amount_quota_user(&db, &am, 10).await;

  let hold: Value = post(&m.client, "/entitlements/reserve", json!({ "action": &action, "amount": 10 }))
    .await
    .json()
    .await
    .unwrap();
  let id = hold["reservationId"].as_i64().unwrap() as i32;
  let mut row = QuotaReservations::find_by_id(id).one(&db).await.unwrap().unwrap().into_active_model();
  row.expires_at = Set(Utc::now() - Duration::hours(1));
  row.update(&db).await.unwrap();

  let r = post(&m.client, &format!("/entitlements/reservations/{id}/capture"), json!({})).await;
  assert_eq!(r.status(), StatusCode::CONFLICT, "an expired hold cannot be captured");
  let r = post(&m.client, "/entitlements/reserve", json!({ "action": &action, "amount": 10 })).await;
  assert_eq!(r.status(), StatusCode::OK, "an expired hold takes no room");

  assert!(maintenance::release_expired_reservations(&db).await.unwrap() >= 1);
  let after = QuotaReservations::find_by_id(id).one(&db).await.unwrap().unwrap();
  assert_eq!(after.status, entity::membership::ReservationStatus::Released);
  let releases = QuotaEntries::find()
    .filter(QuotaEntriesColumn::ReservationId.eq(id))
    .filter(QuotaEntriesColumn::Kind.eq(entity::membership::QuotaEntryKind::Release))
    .count(&db)
    .await
    .unwrap();
  assert_eq!(releases, 1, "the scheduler's release is recorded in the quota ledger");
}
