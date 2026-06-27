//! Subscription quota metering (`plan_actions.quota_per_period`).
//!
//! The entitlement subscription path counts uses per period and falls through
//! (to credits, or denial) once the quota is spent; renewal resets it.

use api::util::random::generate_url_safe_string;
use e2e::prelude::*;
use reqwest::StatusCode;
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
  client.post("/entitlements/check").json(&json!({ "action": action })).send().await.unwrap()
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
  assert_eq!(m.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap().status(), StatusCode::CREATED);

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
  assert_eq!(m.client.post("/subscriptions").json(&json!({ "planCode": sub_code })).send().await.unwrap().status(), StatusCode::CREATED);
  am.client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": u.model.id, "amount": 100, "planCode": credit_code }))
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
  let created: Value =
    m.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap().json().await.unwrap();
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
