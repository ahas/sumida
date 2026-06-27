//! Coupons + tax at checkout (the payment money path).

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

async fn user_mock(db: &sea_orm::DatabaseConnection) -> Mock {
  let m = Mock::new().await;
  let u = TestUser::create(db).await.unwrap();
  u.login(&m.client).await;
  m
}

async fn create_sub_plan(client: &TestClient, price: i64) -> String {
  let resp = client
    .post("/membership-plans")
    .json(&json!({
      "code": format!("cpn-{}", rnd()), "name": "P", "kind": "subscription", "scope": "personal",
      "price": price, "durationDays": 30, "actions": [{ "action": "a" }],
    }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  resp.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string()
}

async fn create_coupon(client: &TestClient, body: Value) -> Value {
  client.post("/coupons").json(&body).send().await.unwrap().json().await.unwrap()
}

fn tax_bp() -> i64 {
  infra::settings::get().membership.tax_percent_bp as i64
}

// ---------------------------------------------------------------------------
// Percent coupon discounts the subtotal; tax is applied on the post-discount amount.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_checkout_percent_coupon_and_tax() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;

  let code = format!("PCT-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "percentOff": 20 })).await;
  let plan = create_sub_plan(&am.client, 1000).await;

  let m = user_mock(&db).await;
  let res: Value = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

  assert_eq!(res["subtotal"].as_i64().unwrap(), 1000);
  assert_eq!(res["discountAmount"].as_i64().unwrap(), 200, "20% of 1000");
  let taxable = 800;
  let expected_tax = taxable * tax_bp() / 10_000;
  assert_eq!(res["taxAmount"].as_i64().unwrap(), expected_tax, "tax on post-discount 800");
  assert_eq!(res["amount"].as_i64().unwrap(), taxable + expected_tax, "total = taxable + tax");
}

// ---------------------------------------------------------------------------
// Fixed-amount coupon requires a matching currency; a mismatch is rejected.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_amount_coupon_currency_match() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await; // currency defaults to KRW

  // Matching currency → applies.
  let ok_code = format!("KRW-{}", rnd());
  create_coupon(&am.client, json!({ "code": ok_code, "amountOff": 300, "currency": "KRW" })).await;
  let m = user_mock(&db).await;
  let res: Value = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": ok_code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(res["discountAmount"].as_i64().unwrap(), 300);

  // Mismatched currency → rejected.
  let bad_code = format!("USD-{}", rnd());
  create_coupon(&am.client, json!({ "code": bad_code, "amountOff": 300, "currency": "USD" })).await;
  let r = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": bad_code }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "currency mismatch must be rejected");
}

// ---------------------------------------------------------------------------
// A coupon's redemption cap is enforced.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_redemption_cap() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("ONCE-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "maxRedemptions": 1 })).await;

  let m = user_mock(&db).await;
  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });

  let first = m.client.post("/payments/checkout").json(&body).send().await.unwrap();
  assert_eq!(first.status(), StatusCode::CREATED, "first redemption succeeds");

  let second = m.client.post("/payments/checkout").json(&body).send().await.unwrap();
  assert_eq!(second.status(), StatusCode::BAD_REQUEST, "redemption cap reached");
}

// ---------------------------------------------------------------------------
// A deactivated coupon is rejected at checkout; delete removes it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_update_and_delete() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("UPD-{}", rnd());
  let created = create_coupon(&am.client, json!({ "code": code, "percentOff": 10 })).await;
  let id = created["id"].as_i64().unwrap();

  // Deactivate → checkout rejects it.
  let r = am.client.patch(&format!("/coupons/{id}")).json(&json!({ "isActive": false })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  let m = user_mock(&db).await;
  let co = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(co.status(), StatusCode::BAD_REQUEST, "inactive coupon rejected");

  // Delete → gone.
  let r = am.client.delete(&format!("/coupons/{id}")).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// A per-owner cap (e.g. once-per-customer) blocks the same owner reusing it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_per_owner_limit() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("ONCE1-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "maxRedemptionsPerOwner": 1 })).await;

  // Same owner: first checkout succeeds, second is blocked.
  let a = user_mock(&db).await;
  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });
  assert_eq!(a.client.post("/payments/checkout").json(&body).send().await.unwrap().status(), StatusCode::CREATED);
  assert_eq!(
    a.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "same owner cannot redeem past the per-owner cap"
  );

  // A different owner can still redeem it.
  let b = user_mock(&db).await;
  assert_eq!(
    b.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED,
    "the per-owner cap is per owner, not global"
  );
}

// ---------------------------------------------------------------------------
// Coupon management is admin-only.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_admin_only() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let m = user_mock(&db).await;
  let r = m
    .client
    .post("/coupons")
    .json(&json!({ "code": format!("X-{}", rnd()), "percentOff": 50 }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "non-admin cannot create coupons");
}

// ---------------------------------------------------------------------------
// Full flow: checkout with coupon → confirm → subscription activated at the
// discounted+taxed total (requires the mock PG provider).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_checkout_confirm_with_coupon() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("FLOW-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "percentOff": 50 })).await;

  let m = user_mock(&db).await;
  let checkout: Value = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let purchase_code = checkout["purchaseCode"].as_str().unwrap().to_string();
  let total = checkout["amount"].as_i64().unwrap();
  assert_eq!(checkout["discountAmount"].as_i64().unwrap(), 500);

  // Mock verifies the total encoded in the order id → activation succeeds.
  let confirm: Value = m
    .client
    .post("/payments/confirm")
    .json(&json!({ "purchaseCode": purchase_code, "paymentId": purchase_code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(confirm["status"].as_str().unwrap(), "completed");
  assert!(confirm["subscriptionId"].as_i64().is_some(), "subscription activated");
  assert!(total < 1000, "charged the discounted total");
}
