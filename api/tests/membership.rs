use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::ap_test_utils::create_ap;
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde_json::{Value, json};

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

/// Create a plan as an authenticated admin client. Returns the plan `code`.
async fn create_plan(client: &TestClient, body: Value) -> String {
  create_plan_full(client, body).await.1
}

/// Create a plan; returns `(id, code)`.
async fn create_plan_full(client: &TestClient, body: Value) -> (i32, String) {
  let resp = client.post("/membership-plans").json(&body).send().await.expect("create plan request");
  assert_eq!(resp.status(), StatusCode::CREATED, "admin should create plan");
  let json: Value = resp.json().await.expect("plan json");
  (json["id"].as_i64().expect("plan id") as i32, json["code"].as_str().expect("plan code").to_string())
}

/// Spawn a fresh server, create a non-admin user, log them in. Returns `(mock, user)`.
async fn new_user_mock(db: &sea_orm::DatabaseConnection) -> (Mock, TestUser) {
  let m = Mock::new().await;
  let u = TestUser::create(db).await.expect("create user");
  u.login(&m.client).await;
  (m, u)
}

/// Subscribe `client` to `plan_code` (personal). Returns the response.
async fn subscribe(client: &TestClient, body: Value) -> reqwest::Response {
  client.post("/subscriptions").json(&body).send().await.expect("subscribe request")
}

// ---------------------------------------------------------------------------
// Plan management is admin-gated (service_ap! on "membership_plan").
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_membership_plan_admin_only() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  // Admin can create.
  let mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&mock.client).await;
  let code = format!("sub-{}", rnd());
  let resp = mock
    .client
    .post("/membership-plans")
    .json(&json!({
      "code": code, "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30,
      "actions": [{ "action": "read-article" }],
    }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);

  // A normal user (no membership_plan policy) cannot create.
  let mock2 = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&mock2.client).await;
  let resp = mock2
    .client
    .post("/membership-plans")
    .json(&json!({
      "code": format!("x-{}", rnd()), "name": "X", "kind": "subscription", "scope": "personal",
    }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "non-admin must not create plans");
}

// ---------------------------------------------------------------------------
// Personal subscription lifecycle + entitlement gate (subscription path).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_personal_subscription_and_entitlement() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("sub-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30,
      "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  // A normal user subscribes to their personal membership.
  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let resp = user_mock.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED, "user subscribes to personal plan");

  // The subscription shows up in their owner-scoped list.
  let resp = user_mock.client.get("/subscriptions").send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let list: Value = resp.json().await.unwrap();
  assert_eq!(list["count"].as_u64().unwrap(), 1);

  // Entitlement gate: covered action passes via subscription.
  let resp = user_mock.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let out: Value = resp.json().await.unwrap();
  assert_eq!(out["via"].as_str().unwrap(), "subscription");

  // Uncovered action: no subscription coverage and no credit plan → denied.
  let resp = user_mock.client.post("/entitlements/check").json(&json!({ "action": "premium-export" })).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "uncovered action must be denied");
}

// ---------------------------------------------------------------------------
// plan.scope must match the subscription owner kind.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_scope_mismatch() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("psub-{}", rnd()), "name": "Personal", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  // Subscribing a personal-scope plan as an organization is rejected.
  let resp = user_mock
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "ownerKind": "organization", "ownerId": user.model.organization_id }))
    .send()
    .await
    .unwrap();
  // Either scope-mismatch (400) — the org-admin gate is also evaluated first and
  // would 403 a non-manager; accept either as "rejected".
  assert!(
    resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::FORBIDDEN,
    "scope mismatch / unauthorized must be rejected, got {}",
    resp.status()
  );
}

// ---------------------------------------------------------------------------
// At most one active subscription per (owner, plan).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_duplicate_active_subscription() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("dup-{}", rnd()), "name": "Dup", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let first = user_mock.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap();
  assert_eq!(first.status(), StatusCode::CREATED);

  let second = user_mock.client.post("/subscriptions").json(&json!({ "planCode": code })).send().await.unwrap();
  assert_eq!(second.status(), StatusCode::CONFLICT, "duplicate active subscription must conflict");
}

// ---------------------------------------------------------------------------
// Credit grant → FIFO consume → idempotency → insufficient (402).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_credit_grant_consume_idempotency_insufficient() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let action = format!("render-{}", rnd());
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("cr-{}", rnd()), "name": "Credits", "kind": "credit", "scope": "personal",
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  let user = TestUser::create(&db).await.unwrap();

  // Admin grants 30 credits to the user.
  let resp = admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 30, "planCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::OK);
  let g: Value = resp.json().await.unwrap();
  assert_eq!(g["balance"].as_i64().unwrap(), 30);

  // User consumes their own credits.
  let user_mock = Mock::new().await;
  user.login(&user_mock.client).await;

  let consume = |key: &str| {
    let c = &user_mock.client;
    let body = json!({ "action": &action, "idempotencyKey": key });
    async move { c.post("/credits/consume").json(&body).send().await.unwrap() }
  };

  // First consume: 30 → 20.
  let r = consume("k1").await;
  assert_eq!(r.status(), StatusCode::OK);
  assert_eq!(r.json::<Value>().await.unwrap()["balance"].as_i64().unwrap(), 20);

  // Idempotent retry with the same key: no double charge, still 20.
  let r = consume("k1").await;
  assert_eq!(r.status(), StatusCode::OK);
  assert_eq!(r.json::<Value>().await.unwrap()["balance"].as_i64().unwrap(), 20);

  // Two more distinct consumes: 20 → 10 → 0.
  assert_eq!(consume("k2").await.json::<Value>().await.unwrap()["balance"].as_i64().unwrap(), 10);
  assert_eq!(consume("k3").await.json::<Value>().await.unwrap()["balance"].as_i64().unwrap(), 0);

  // Now insufficient → 402 Payment Required.
  let r = consume("k4").await;
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "insufficient credits → 402");
}

// ---------------------------------------------------------------------------
// Organization subscription requires an org-admin (`org:subscription`) policy;
// hierarchy delegation is provided by the org-ancestors subquery.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_org_subscription_requires_org_admin() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("org-{}", rnd()), "name": "Team", "kind": "subscription", "scope": "organization",
      "durationDays": 30, "actions": [{ "action": "team-feature" }],
    }),
  )
  .await;

  let org = TestOrg::create(&db).await.unwrap();

  // A plain org member (no policy) cannot manage the org's subscription.
  let member_mock = Mock::new().await;
  let member = TestUser::create_in_org(&db, org.model.id).await.unwrap();
  member.login(&member_mock.client).await;
  let resp = member_mock
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "ownerKind": "organization", "ownerId": org.model.id }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "non-admin org member must be denied");

  // Grant the member an org-admin policy for this org and retry.
  create_ap(&db, &member, "org:subscription", &org.model.id.to_string(), vec!["manage".to_string()], "allow")
    .await
    .unwrap();

  let admin_member_mock = Mock::new().await;
  member.login(&admin_member_mock.client).await;
  let resp = admin_member_mock
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": code, "ownerKind": "organization", "ownerId": org.model.id }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED, "org-admin must be able to subscribe the org");
}

// ---------------------------------------------------------------------------
// Owner scoping: a user cannot read another owner's credit ledger.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_credit_ledger_owner_scoping() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("cr2-{}", rnd()), "name": "Credits", "kind": "credit", "scope": "personal",
      "actions": [{ "action": format!("scope-{}", rnd()), "creditCost": 5 }],
    }),
  )
  .await;

  let owner = TestUser::create(&db).await.unwrap();
  let grant = admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": owner.model.id, "amount": 50, "planCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(grant.status(), StatusCode::OK);
  let wallet_id = grant.json::<Value>().await.unwrap()["walletId"].as_i64().unwrap();

  // A different, unrelated user must not read the owner's ledger.
  let other_mock = Mock::new().await;
  let other = TestUser::create(&db).await.unwrap();
  other.login(&other_mock.client).await;
  let resp = other_mock
    .client
    .get(&format!("/credits/transactions?walletId={wallet_id}"))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "cross-owner ledger read must be forbidden");
}

// ---------------------------------------------------------------------------
// Immediate cancel → status canceled, entitlement stops, audit event recorded.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_cancel_immediate() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("c-{}", rnd()), "name": "C", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  let (user_mock, _user) = new_user_mock(&db).await;
  let created: Value = subscribe(&user_mock.client, json!({ "planCode": code })).await.json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Entitlement works while active.
  let r = user_mock.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  // Cancel immediately.
  let r = user_mock.client.post(&format!("/subscriptions/{sub_id}/cancel")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  // Status is canceled and entitlement no longer passes via subscription.
  let got: Value = user_mock.client.get(&format!("/subscriptions/{sub_id}")).send().await.unwrap().json().await.unwrap();
  assert_eq!(got["status"].as_str().unwrap(), "canceled");

  let r = user_mock.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "canceled subscription must not entitle");

  // Audit event recorded.
  let events = SubscriptionEvents::find()
    .filter(entity::membership::subscription_events::Column::SubscriptionId.eq(sub_id))
    .filter(entity::membership::subscription_events::Column::NewStatus.eq("canceled"))
    .all(&db)
    .await
    .unwrap();
  assert!(!events.is_empty(), "a canceled subscription_event must be recorded");
}

// ---------------------------------------------------------------------------
// Cancel-at-period-end keeps the subscription active until the period ends.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_cancel_at_period_end() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("cpe-{}", rnd()), "name": "C", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  let (user_mock, _user) = new_user_mock(&db).await;
  let created: Value = subscribe(&user_mock.client, json!({ "planCode": code })).await.json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  let r = user_mock.client.post(&format!("/subscriptions/{sub_id}/cancel?atPeriodEnd=true")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  let got: Value = user_mock.client.get(&format!("/subscriptions/{sub_id}")).send().await.unwrap().json().await.unwrap();
  assert_eq!(got["status"].as_str().unwrap(), "active", "still active until period end");
  assert_eq!(got["cancelAtPeriodEnd"].as_bool().unwrap(), true);

  // Still entitled during the remaining period.
  let r = user_mock.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK, "still entitled until period end");
}

// ---------------------------------------------------------------------------
// Renew extends current_period_end.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_renew_extends_period() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("rn-{}", rnd()), "name": "R", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (user_mock, _user) = new_user_mock(&db).await;
  let created: Value = subscribe(&user_mock.client, json!({ "planCode": code })).await.json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  let before: Value = user_mock.client.get(&format!("/subscriptions/{sub_id}")).send().await.unwrap().json().await.unwrap();
  let end_before = before["currentPeriodEnd"].as_str().unwrap().to_string();

  let r = user_mock.client.post(&format!("/subscriptions/{sub_id}/renew")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  let after: Value = user_mock.client.get(&format!("/subscriptions/{sub_id}")).send().await.unwrap().json().await.unwrap();
  let end_after = after["currentPeriodEnd"].as_str().unwrap().to_string();
  assert!(end_after > end_before, "renew must push current_period_end forward ({end_before} -> {end_after})");
}

// ---------------------------------------------------------------------------
// Idempotent subscribe: same key returns the same subscription, not a 409.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_idempotent() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("idm-{}", rnd()), "name": "I", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (user_mock, _user) = new_user_mock(&db).await;
  let key = format!("idem-{}", rnd());

  let r1 = subscribe(&user_mock.client, json!({ "planCode": code, "idempotencyKey": key })).await;
  assert_eq!(r1.status(), StatusCode::CREATED);
  let id1 = r1.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  let r2 = subscribe(&user_mock.client, json!({ "planCode": code, "idempotencyKey": key })).await;
  assert_eq!(r2.status(), StatusCode::CREATED, "idempotent replay must not 409");
  let id2 = r2.json::<Value>().await.unwrap()["id"].as_i64().unwrap();

  assert_eq!(id1, id2, "same idempotency key must return the same subscription");
}

// ---------------------------------------------------------------------------
// A user cannot read another user's subscription.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_get_cross_owner_forbidden() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("xo-{}", rnd()), "name": "X", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (a_mock, _a) = new_user_mock(&db).await;
  let created: Value = subscribe(&a_mock.client, json!({ "planCode": code })).await.json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap();

  let (b_mock, _b) = new_user_mock(&db).await;
  let resp = b_mock.client.get(&format!("/subscriptions/{sub_id}")).send().await.unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "another user must not read the subscription");
}

// ---------------------------------------------------------------------------
// Refund reduces balance; refunding more than the balance is rejected (400).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_credit_refund_and_exceeds() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("rf-{}", rnd()), "name": "Cr", "kind": "credit", "scope": "personal",
      "actions": [{ "action": "x", "creditCost": 1 }],
    }),
  )
  .await;

  let user = TestUser::create(&db).await.unwrap();
  admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 50, "planCode": code }))
    .send()
    .await
    .unwrap();

  // Refund 20 → 30 remaining.
  let r = admin_mock
    .client
    .post("/credits/refund")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 20 }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK);
  assert_eq!(r.json::<Value>().await.unwrap()["balance"].as_i64().unwrap(), 30);

  // Refund 100 (exceeds) → 400.
  let r = admin_mock
    .client
    .post("/credits/refund")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 100 }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "refund exceeding balance must be rejected");
}

// ---------------------------------------------------------------------------
// Expired credit lots are excluded from balance and consumption (FIFO sweep).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_credit_expired_lots_excluded() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let action = format!("render-{}", rnd());
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("ex-{}", rnd()), "name": "Cr", "kind": "credit", "scope": "personal",
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  let user = TestUser::create(&db).await.unwrap();
  let past = (Utc::now() - Duration::hours(1)).to_rfc3339();

  // 50 already-expired + 30 fresh.
  admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 50, "planCode": code, "expiryAt": past }))
    .send()
    .await
    .unwrap();
  admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 30, "planCode": code }))
    .send()
    .await
    .unwrap();

  let (user_mock, _) = (Mock::new().await, ());
  user.login(&user_mock.client).await;

  // Balance reflects only the non-expired 30.
  let bal: Value = user_mock.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 30, "expired lot must be swept from balance");

  // Three consumes (10 each) succeed; the fourth hits insufficient (expired 50 unusable).
  for _ in 0..3 {
    let r = user_mock.client.post("/credits/consume").json(&json!({ "action": &action })).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
  }
  let r = user_mock.client.post("/credits/consume").json(&json!({ "action": &action })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "expired credits must not be spendable");
}

// ---------------------------------------------------------------------------
// An expired subscription period stops entitling (current_period_end is authoritative).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_subscription_expiry_denies_entitlement() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("exp-{}", rnd()), "name": "E", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  let (user_mock, _user) = new_user_mock(&db).await;
  let created: Value = subscribe(&user_mock.client, json!({ "planCode": code })).await.json().await.unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;

  // Entitled now.
  let r = user_mock.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  // Force the period to have ended.
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  let mut am = sub.into_active_model();
  am.current_period_end = Set((Utc::now() - Duration::hours(1)).into());
  am.update(&db).await.unwrap();

  // No longer entitled (and no credit plan covers it).
  let r = user_mock.client.post("/entitlements/check").json(&json!({ "action": "read-article" })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "expired period must stop entitlement");
}

// ---------------------------------------------------------------------------
// Entitlement gate consumes credits when there is no covering subscription.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_entitlement_credit_path() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let action = format!("render-{}", rnd());
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("ecp-{}", rnd()), "name": "Cr", "kind": "credit", "scope": "personal",
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  let user = TestUser::create(&db).await.unwrap();
  admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 100, "planCode": code }))
    .send()
    .await
    .unwrap();

  let user_mock = Mock::new().await;
  user.login(&user_mock.client).await;

  // No subscription → gate falls through to credits and consumes 10.
  let r = user_mock.client.post("/entitlements/check").json(&json!({ "action": &action })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);
  let out: Value = r.json().await.unwrap();
  assert_eq!(out["via"].as_str().unwrap(), "credit");
  assert_eq!(out["consumed"].as_i64().unwrap(), 10);

  let bal: Value = user_mock.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 90);
}

// ---------------------------------------------------------------------------
// Organization credit: admin grants to org, same-org member uses it, outsiders denied.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_org_credit_grant_consume_scoping() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let action = format!("render-{}", rnd());
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("oc-{}", rnd()), "name": "OrgCr", "kind": "credit", "scope": "organization",
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  let org = TestOrg::create(&db).await.unwrap();
  let r = admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "organization", "ownerId": org.model.id, "amount": 50, "planCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  // A member of the org can consume the org's credits.
  let member_mock = Mock::new().await;
  let member = TestUser::create_in_org(&db, org.model.id).await.unwrap();
  member.login(&member_mock.client).await;
  let r = member_mock
    .client
    .post("/credits/consume")
    .json(&json!({ "action": &action, "ownerKind": "organization", "ownerId": org.model.id }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK, "same-org member can spend org credits");

  // An unrelated user (different org, no policy) cannot.
  let (out_mock, _out) = new_user_mock(&db).await;
  let r = out_mock
    .client
    .post("/credits/consume")
    .json(&json!({ "action": &action, "ownerKind": "organization", "ownerId": org.model.id }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "outsider must not spend org credits");
}

// ---------------------------------------------------------------------------
// Soft-deleting a plan retires it: new subscriptions can't reference it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_plan_soft_delete_blocks_subscribe() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let (plan_id, code) = create_plan_full(
    &admin_mock.client,
    json!({
      "code": format!("sd-{}", rnd()), "name": "SD", "kind": "subscription", "scope": "personal",
      "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  // Retire it.
  let r = admin_mock.client.delete(&format!("/membership-plans/{plan_id}")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  // Subscribing to the retired plan now fails with 404.
  let (user_mock, _user) = new_user_mock(&db).await;
  let r = subscribe(&user_mock.client, json!({ "planCode": code })).await;
  assert_eq!(r.status(), StatusCode::NOT_FOUND, "retired plan must not be subscribable");
}

// ---------------------------------------------------------------------------
// The credit ledger records grant + consume rows with balance_after.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_credit_transactions_ledger() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let action = format!("render-{}", rnd());
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("lg-{}", rnd()), "name": "Cr", "kind": "credit", "scope": "personal",
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  let user = TestUser::create(&db).await.unwrap();
  let grant: Value = admin_mock
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": 50, "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let wallet_id = grant["walletId"].as_i64().unwrap();

  let user_mock = Mock::new().await;
  user.login(&user_mock.client).await;
  user_mock.client.post("/credits/consume").json(&json!({ "action": &action })).send().await.unwrap();

  // Owner reads their ledger: a grant (+50) and a consume (-10).
  let list: Value = user_mock
    .client
    .get(&format!("/credits/transactions?walletId={wallet_id}"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert!(list["count"].as_u64().unwrap() >= 2, "ledger must contain grant + consume");
  let kinds: Vec<&str> = list["items"].as_array().unwrap().iter().filter_map(|i| i["kind"].as_str()).collect();
  assert!(kinds.contains(&"grant") && kinds.contains(&"consume"), "ledger kinds: {kinds:?}");
}

// ---------------------------------------------------------------------------
// Subscribing records a canonical purchase, retrievable by the owner.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_membership_purchase_tracked() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pt-{}", rnd()), "name": "P", "kind": "subscription", "scope": "personal",
      "price": 1500, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let (user_mock, _user) = new_user_mock(&db).await;
  assert_eq!(subscribe(&user_mock.client, json!({ "planCode": code })).await.status(), StatusCode::CREATED);

  let list: Value = user_mock.client.get("/membership-purchases").send().await.unwrap().json().await.unwrap();
  assert_eq!(list["count"].as_u64().unwrap(), 1, "one purchase tracked");
  let item = &list["items"][0];
  assert_eq!(item["kind"].as_str().unwrap(), "subscription");
  assert_eq!(item["status"].as_str().unwrap(), "completed");
  assert_eq!(item["planCode"].as_str().unwrap(), code);
  assert!(item["code"].as_str().is_some(), "purchase has a tracking code");
}
