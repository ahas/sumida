//! E2E for the PG-backed payment flow, exercised against the deterministic
//! `mock` payment provider (settings.e2e.toml → `[membership.payment] provider = "mock"`).
//!
//! The mock encodes the outcome in the order id / payment id:
//! `mock:paid:<amount>:<currency>` → PAID, `mock:failed` → FAILED. Checkout generates a
//! `mock:paid:<price>:<currency>:…` order code, so the happy path verifies a matching amount. A
//! purchase is only ever verified against its own code, so a decline or a short payment is
//! scripted for that code with `infra::payment::mock::script`.

use api::util::random::generate_url_safe_string;
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{Value, json};

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

async fn create_plan(client: &TestClient, body: Value) -> String {
  let resp = client.post("/membership-plans").json(&body).send().await.expect("create plan");
  assert_eq!(resp.status(), StatusCode::CREATED, "admin creates plan");
  resp.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string()
}

async fn checkout(client: &TestClient, body: Value) -> Value {
  let resp = client.post("/payments/checkout").json(&body).send().await.expect("checkout");
  assert_eq!(resp.status(), StatusCode::CREATED, "checkout creates a pending purchase");
  resp.json().await.unwrap()
}

async fn confirm(client: &TestClient, purchase_code: &str, payment_id: &str) -> reqwest::Response {
  client
    .post("/payments/confirm")
    .json(&json!({ "purchaseCode": purchase_code, "paymentId": payment_id }))
    .send()
    .await
    .expect("confirm")
}

// ---------------------------------------------------------------------------
// checkout → confirm activates a subscription; purchase is completed.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_subscription_checkout_confirm() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pp-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "read-article" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();
  // amount = price + tax (no coupon); tax depends on the configured rate.
  let tax_bp = infra::settings::get().membership.tax_percent_bp as i64;
  assert_eq!(co["amount"].as_i64().unwrap(), 1000 + 1000 * tax_bp / 10_000);
  assert_eq!(co["provider"].as_str().unwrap(), "mock");

  // Happy path: paymentId = purchaseCode (mock order code encodes PAID 1000).
  let cf = confirm(&user_mock.client, &purchase_code, &purchase_code).await;
  assert_eq!(cf.status(), StatusCode::OK);
  let cfj: Value = cf.json().await.unwrap();
  assert_eq!(cfj["status"].as_str().unwrap(), "completed");
  assert!(cfj["subscriptionId"].as_i64().is_some(), "subscription activated");

  // Subscription is active and entitlement passes.
  let subs: Value = user_mock.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  assert_eq!(subs["count"].as_u64().unwrap(), 1);
  assert_eq!(subs["items"][0]["status"].as_str().unwrap(), "active");

  let ent = user_mock
    .client
    .post("/entitlements/check")
    .json(&json!({ "action": "read-article" }))
    .send()
    .await
    .unwrap();
  assert_eq!(ent.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Amount mismatch is rejected (server-side verification guards tampering).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_amount_mismatch_rejected() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pm-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();

  // Verified amount (500) != expected (1000) → 400.
  infra::payment::mock::script(&purchase_code, "mock:paid:500:KRW");
  let cf = confirm(&user_mock.client, &purchase_code, &purchase_code).await;
  assert_eq!(cf.status(), StatusCode::BAD_REQUEST, "amount mismatch must be rejected");

  // No subscription was created.
  let subs: Value = user_mock.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  assert_eq!(subs["count"].as_u64().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// A non-PAID payment is rejected with 402.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_failed_status_rejected() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pf-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();

  infra::payment::mock::script(&purchase_code, "mock:failed");
  let cf = confirm(&user_mock.client, &purchase_code, &purchase_code).await;
  assert_eq!(cf.status(), StatusCode::PAYMENT_REQUIRED, "unpaid payment → 402");
}

// ---------------------------------------------------------------------------
// Credit plan purchase grants the plan's credit units.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_credit_purchase_grants_units() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pc-{}", rnd()), "name": "100 credits", "kind": "credit", "scope": "personal",
      "price": 5000, "creditAmount": 100, "actions": [{ "action": "render", "creditCost": 10 }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "credit", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();
  let tax_bp = infra::settings::get().membership.tax_percent_bp as i64;
  assert_eq!(co["amount"].as_i64().unwrap(), 5000 + 5000 * tax_bp / 10_000, "charge = price + tax");

  let cf = confirm(&user_mock.client, &purchase_code, &purchase_code).await;
  assert_eq!(cf.status(), StatusCode::OK);

  // 100 credit units granted (not the 5000 money amount).
  let bal: Value = user_mock.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 100);
}

// ---------------------------------------------------------------------------
// Webhook activates the purchase and is idempotent on redelivery.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_webhook_activates_idempotent() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pw-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();

  let webhook_id = format!("wh-{}", rnd());
  let body = json!({ "type": "Transaction.Paid", "data": { "paymentId": purchase_code } }).to_string();

  // Deliver twice with the same webhook id → second is a no-op.
  for _ in 0..2 {
    let resp = user_mock
      .client
      .post("/payments/webhook")
      .header("webhook-id", &webhook_id)
      .header("webhook-timestamp", "1700000000")
      .header("content-type", "application/json")
      .body(body.clone())
      .send()
      .await
      .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "webhook accepted");
  }

  // Exactly one active subscription was created.
  let subs: Value = user_mock.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  assert_eq!(subs["count"].as_u64().unwrap(), 1, "idempotent: no duplicate activation");
  assert_eq!(subs["items"][0]["status"].as_str().unwrap(), "active");
}

// ---------------------------------------------------------------------------
// Refund: provider cancel + subscription canceled + purchase refunded.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_refund_subscription() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pr-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();
  assert_eq!(
    confirm(&user_mock.client, &purchase_code, &purchase_code).await.status(),
    StatusCode::OK
  );

  // Refund.
  let rf = user_mock
    .client
    .post("/payments/refund")
    .json(&json!({ "purchaseCode": purchase_code, "reason": "customer request" }))
    .send()
    .await
    .unwrap();
  assert_eq!(rf.status(), StatusCode::OK);
  assert_eq!(rf.json::<Value>().await.unwrap()["status"].as_str().unwrap(), "refunded");

  // Subscription is canceled; purchase is refunded.
  let subs: Value = user_mock.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  assert_eq!(subs["items"][0]["status"].as_str().unwrap(), "canceled");

  let purchase = MembershipPurchases::find()
    .filter(entity::membership::membership_purchases::Column::Code.eq(purchase_code))
    .one(&db)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(format!("{:?}", purchase.status), "Refunded");
}

// ---------------------------------------------------------------------------
// Refunding a credit purchase claws back the granted units.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_refund_credit_clawback() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("prc-{}", rnd()), "name": "Credits", "kind": "credit", "scope": "personal",
      "price": 5000, "creditAmount": 100, "actions": [{ "action": "x", "creditCost": 1 }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "credit", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();
  assert_eq!(
    confirm(&user_mock.client, &purchase_code, &purchase_code).await.status(),
    StatusCode::OK
  );

  let bal: Value = user_mock.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 100);

  // Refund → 100 units clawed back → balance 0.
  let rf = user_mock
    .client
    .post("/payments/refund")
    .json(&json!({ "purchaseCode": purchase_code }))
    .send()
    .await
    .unwrap();
  assert_eq!(rf.status(), StatusCode::OK);

  let bal: Value = user_mock.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 0, "granted credits clawed back");
}

// ---------------------------------------------------------------------------
// Organization checkout requires an org-manager policy (ReBAC).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_org_checkout_requires_manager() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("po-{}", rnd()), "name": "Team", "kind": "subscription", "scope": "organization",
      "price": 9000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let org = TestOrg::create(&db).await.unwrap();
  let member_mock = Mock::new().await;
  let member = TestUser::create_in_org(&db, org.model.id).await.unwrap();
  member.login(&member_mock.client).await;

  let resp = member_mock
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": code, "ownerKind": "organization", "ownerId": org.model.id }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::FORBIDDEN, "non-manager cannot checkout for the org");
}

async fn purchase_status(db: &sea_orm::DatabaseConnection, code: &str) -> String {
  let purchase = MembershipPurchases::find()
    .filter(entity::membership::membership_purchases::Column::Code.eq(code))
    .one(db)
    .await
    .unwrap()
    .unwrap();
  format!("{:?}", purchase.status)
}

// ---------------------------------------------------------------------------
// One real payment settles exactly one purchase: confirming a second pending purchase with the
// first one's payment id is refused, and the second stays pending with nothing granted.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_confirm_refuses_another_purchases_payment() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pb-{}", rnd()), "name": "Credits", "kind": "credit", "scope": "personal",
      "price": 5000, "creditAmount": 100, "actions": [{ "action": "x", "creditCost": 1 }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let paid = checkout(&user_mock.client, json!({ "kind": "credit", "planCode": code })).await;
  let paid = paid["purchaseCode"].as_str().unwrap().to_string();
  let unpaid = checkout(&user_mock.client, json!({ "kind": "credit", "planCode": code })).await;
  let unpaid = unpaid["purchaseCode"].as_str().unwrap().to_string();
  // The second purchase was never paid at the PG.
  infra::payment::mock::script(&unpaid, "mock:pending");

  assert_eq!(confirm(&user_mock.client, &paid, &paid).await.status(), StatusCode::OK);

  // The paid purchase's payment id offered for the unpaid one: refused before the PG is asked.
  let cf = confirm(&user_mock.client, &unpaid, &paid).await;
  assert_eq!(cf.status(), StatusCode::BAD_REQUEST, "a payment binds to its own purchase only");
  // Leaving the id out verifies the unpaid purchase's own payment, which has not been paid.
  let cf = user_mock
    .client
    .post("/payments/confirm")
    .json(&json!({ "purchaseCode": unpaid }))
    .send()
    .await
    .unwrap();
  assert_eq!(cf.status(), StatusCode::PAYMENT_REQUIRED);

  assert_eq!(
    purchase_status(&db, &unpaid).await,
    "Pending",
    "an unfinished payment leaves it pending"
  );
  let bal: Value = user_mock.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 100, "only the one paid purchase granted credits");
}

// ---------------------------------------------------------------------------
// A payment in another currency than the purchase's does not settle it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_currency_mismatch_rejected() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pcur-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "currency": "KRW", "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();
  let amount = co["amount"].as_i64().unwrap();
  infra::payment::mock::script(&purchase_code, &format!("mock:paid:{amount}:USD"));

  let cf = confirm(&user_mock.client, &purchase_code, &purchase_code).await;
  assert_eq!(cf.status(), StatusCode::BAD_REQUEST, "same number in another currency is not the price");
  let subs: Value = user_mock.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  assert_eq!(subs["count"].as_u64().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// A failed purchase stays failed: once a decline has given its coupons back, a later confirm of the
// same purchase — even after the PG reports it paid — is refused and grants nothing.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_payment_failed_purchase_is_not_settled_later() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;

  let admin_mock = Mock::new().await;
  let admin = TestUser::create_admin(&db).await.unwrap();
  admin.login(&admin_mock.client).await;
  let code = create_plan(
    &admin_mock.client,
    json!({
      "code": format!("pfl-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;

  let user_mock = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&user_mock.client).await;

  let co = checkout(&user_mock.client, json!({ "kind": "subscription", "planCode": code })).await;
  let purchase_code = co["purchaseCode"].as_str().unwrap().to_string();

  infra::payment::mock::script(&purchase_code, "mock:failed");
  assert_eq!(
    confirm(&user_mock.client, &purchase_code, &purchase_code).await.status(),
    StatusCode::PAYMENT_REQUIRED
  );
  assert_eq!(purchase_status(&db, &purchase_code).await, "Failed");

  // The PG now answers PAID for the same payment: the failed purchase is still not settled.
  infra::payment::mock::unscript(&purchase_code);
  let cf = confirm(&user_mock.client, &purchase_code, &purchase_code).await;
  assert_eq!(cf.status(), StatusCode::CONFLICT, "only a pending purchase is settled");
  assert_eq!(purchase_status(&db, &purchase_code).await, "Failed");
  let subs: Value = user_mock.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  assert_eq!(subs["count"].as_u64().unwrap(), 0, "nothing activated");
}
