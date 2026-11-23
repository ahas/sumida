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
// A currency-scoped discount (regional promo) applies only to that currency.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_currency_scope() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await; // currency defaults to KRW
  let m = user_mock(&db).await;

  // Scoped to the plan's currency (KRW) → applies.
  let ok_code = format!("KRW-{}", rnd());
  create_coupon(&am.client, json!({ "code": ok_code, "percentOff": 20, "currency": "KRW" })).await;
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
  assert_eq!(res["discountAmount"].as_i64().unwrap(), 200, "20% of 1000");

  // Scoped to a different currency (USD) → rejected for a KRW purchase.
  let bad_code = format!("USD-{}", rnd());
  create_coupon(&am.client, json!({ "code": bad_code, "percentOff": 20, "currency": "USD" })).await;
  let r = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": bad_code }))
    .send()
    .await
    .unwrap();
  assert_eq!(
    r.status(),
    StatusCode::BAD_REQUEST,
    "currency-scoped coupon rejected for other currency"
  );
}

// ---------------------------------------------------------------------------
// A reward coupon grants credits on redeem (and is rejected at checkout).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_redeem_credit_coupon() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("CR-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "grantCredits": 250 })).await;

  let m = user_mock(&db).await;
  // Reward coupons can't be used as a checkout discount.
  let at_checkout = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(at_checkout.status(), StatusCode::BAD_REQUEST, "reward coupon is not a discount");

  // Redeem → credits land in the wallet.
  let r = m.client.post("/coupons/redeem").json(&json!({ "code": code })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);
  let body: Value = r.json().await.unwrap();
  assert_eq!(body["reward"].as_str().unwrap(), "credits");
  assert_eq!(body["creditsGranted"].as_i64().unwrap(), 250);
  let bal: Value = m.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 250);
}

// ---------------------------------------------------------------------------
// A reward coupon grants a free subscription period on redeem.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_redeem_free_plan_coupon() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("FREE-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "freePlanCode": plan })).await;

  let m = user_mock(&db).await;
  let r = m.client.post("/coupons/redeem").json(&json!({ "code": code })).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::OK);
  let body: Value = r.json().await.unwrap();
  assert_eq!(body["reward"].as_str().unwrap(), "free-plan");
  let sub_id = body["subscriptionId"].as_i64().unwrap();

  // The granted subscription is active and free.
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
  assert_eq!(got["planCode"].as_str().unwrap(), plan);
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
  let r = am
    .client
    .patch(&format!("/coupons/{id}"))
    .json(&json!({ "isActive": false }))
    .send()
    .await
    .unwrap();
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
  assert_eq!(
    a.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED
  );
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
// The per-owner cap can be set via PATCH (not only at creation).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_update_sets_per_owner_cap() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("UPO-{}", rnd());
  let created = create_coupon(&am.client, json!({ "code": code, "percentOff": 10 })).await;
  let id = created["id"].as_i64().unwrap();

  // Tighten to once-per-owner via update.
  let r = am
    .client
    .patch(&format!("/coupons/{id}"))
    .json(&json!({ "maxRedemptionsPerOwner": 1 }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  let m = user_mock(&db).await;
  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });
  assert_eq!(
    m.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED
  );
  assert_eq!(
    m.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "update applied the per-owner cap"
  );
}

// ---------------------------------------------------------------------------
// A grant-gated coupon is redeemable only by owners it was issued to.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_requires_grant_targeting() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("EXC-{}", rnd());
  let created =
    create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "requiresGrant": true })).await;
  let coupon_id = created["id"].as_i64().unwrap();
  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });

  // User A: not granted → rejected.
  let user_a = TestUser::create(&db).await.unwrap();
  let ma = Mock::new().await;
  user_a.login(&ma.client).await;
  assert_eq!(
    ma.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "ungranted owner cannot redeem a grant-gated coupon"
  );

  // Admin issues it to A → A can now redeem.
  let g = am
    .client
    .post(&format!("/coupons/{coupon_id}/grant"))
    .json(&json!({ "ownerKind": "personal", "ownerId": user_a.model.id }))
    .send()
    .await
    .unwrap();
  assert_eq!(g.status(), StatusCode::CREATED);
  assert_eq!(
    ma.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED
  );

  // User B: still not granted → rejected.
  let mb = user_mock(&db).await;
  assert_eq!(
    mb.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "the grant is per owner"
  );
}

// ---------------------------------------------------------------------------
// Grant lifecycle: one grant = one use; recall blocks; re-issue needs a revoke.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_grant_lifecycle() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("LC-{}", rnd());
  let created =
    create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "requiresGrant": true })).await;
  let cid = created["id"].as_i64().unwrap();
  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });

  let user = TestUser::create(&db).await.unwrap();
  let m = Mock::new().await;
  user.login(&m.client).await;
  let uid = user.model.id;
  let grant = || {
    am.client
      .post(&format!("/coupons/{cid}/grant"))
      .json(&json!({ "ownerKind": "personal", "ownerId": uid }))
  };

  // Issue → one redemption succeeds, then the grant is spent.
  assert_eq!(grant().send().await.unwrap().status(), StatusCode::CREATED);
  assert_eq!(
    m.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED
  );
  // It also disappears from "my coupons" (redeemed).
  let mine: Value = m.client.get("/coupons/mine").send().await.unwrap().json().await.unwrap();
  assert!(
    !mine["items"]
      .as_array()
      .unwrap()
      .iter()
      .any(|i| i["code"].as_str() == Some(code.as_str()))
  );
  // Second use blocked; re-grant blocked (must revoke first).
  assert_eq!(
    m.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST
  );
  assert_eq!(
    grant().send().await.unwrap().status(),
    StatusCode::CONFLICT,
    "no silent re-issue of a redeemed grant"
  );

  // Revoke, then re-issue (allowed), then it works again.
  let rv = am
    .client
    .delete(&format!("/coupons/{cid}/grant?ownerKind=personal&ownerId={uid}"))
    .send()
    .await
    .unwrap();
  assert_eq!(rv.status(), StatusCode::OK);
  assert_eq!(grant().send().await.unwrap().status(), StatusCode::CREATED, "re-issue after revoke");
  assert_eq!(
    m.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::CREATED
  );
}

// ---------------------------------------------------------------------------
// A revoked grant blocks redemption immediately.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_revoke_blocks_redemption() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("RV-{}", rnd());
  let cid = create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "requiresGrant": true }))
    .await["id"]
    .as_i64()
    .unwrap();

  let user = TestUser::create(&db).await.unwrap();
  let m = Mock::new().await;
  user.login(&m.client).await;
  let uid = user.model.id;

  am.client
    .post(&format!("/coupons/{cid}/grant"))
    .json(&json!({ "ownerId": uid }))
    .send()
    .await
    .unwrap();
  am.client.delete(&format!("/coupons/{cid}/grant?ownerId={uid}")).send().await.unwrap();

  let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });
  assert_eq!(
    m.client.post("/payments/checkout").json(&body).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "revoked grant cannot be redeemed"
  );
}

// ---------------------------------------------------------------------------
// Admin can list a coupon's grants (issued/redeemed/revoked analytics).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_grants_list() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = format!("GL-{}", rnd());
  let cid = create_coupon(&am.client, json!({ "code": code, "percentOff": 10 })).await["id"]
    .as_i64()
    .unwrap();

  let a = TestUser::create(&db).await.unwrap();
  let b = TestUser::create(&db).await.unwrap();
  for uid in [a.model.id, b.model.id] {
    am.client
      .post(&format!("/coupons/{cid}/grant"))
      .json(&json!({ "ownerId": uid }))
      .send()
      .await
      .unwrap();
  }

  let list: Value = am
    .client
    .get(&format!("/coupons/{cid}/grants"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(list["count"].as_u64().unwrap(), 2, "both grants are listed");
}

// ---------------------------------------------------------------------------
// "My coupons" lists coupons issued to the caller.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_my_coupons() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = format!("MINE-{}", rnd());
  let created = create_coupon(&am.client, json!({ "code": code, "percentOff": 15 })).await;
  let coupon_id = created["id"].as_i64().unwrap();

  let user = TestUser::create(&db).await.unwrap();
  let m = Mock::new().await;
  user.login(&m.client).await;

  // Before issuance: not in the user's list.
  let before: Value = m.client.get("/coupons/mine").send().await.unwrap().json().await.unwrap();
  let has =
    |v: &Value| v["items"].as_array().unwrap().iter().any(|i| i["code"].as_str() == Some(code.as_str()));
  assert!(!has(&before), "coupon not yet issued to the user");

  // Issue, then it appears.
  am.client
    .post(&format!("/coupons/{coupon_id}/grant"))
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id }))
    .send()
    .await
    .unwrap();
  let after: Value = m.client.get("/coupons/mine").send().await.unwrap().json().await.unwrap();
  assert!(has(&after), "issued coupon shows in the user's coupons");

  // Another user does not see it.
  let other = user_mock(&db).await;
  let other_list: Value = other.client.get("/coupons/mine").send().await.unwrap().json().await.unwrap();
  assert!(!has(&other_list), "a different user does not see the coupon");
}

// ---------------------------------------------------------------------------
// A plan-scoped coupon applies only to its listed plans.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_plan_scoped() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan_a = create_sub_plan(&am.client, 1000).await;
  let plan_b = create_sub_plan(&am.client, 1000).await;
  let code = format!("PS-{}", rnd());
  create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "planCodes": [plan_a] })).await;

  let m = user_mock(&db).await;
  // Allowed plan → applies.
  let ok = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan_a, "couponCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(ok.status(), StatusCode::CREATED);
  assert_eq!(ok.json::<Value>().await.unwrap()["discountAmount"].as_i64().unwrap(), 100);

  // Other plan → rejected.
  let bad = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan_b, "couponCode": code }))
    .send()
    .await
    .unwrap();
  assert_eq!(bad.status(), StatusCode::BAD_REQUEST, "coupon not valid for an unlisted plan");
}

// ---------------------------------------------------------------------------
// Stackable coupons combine; a non-stackable one cannot be mixed.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_stacking() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let c10 = format!("S10-{}", rnd());
  let c20 = format!("S20-{}", rnd());
  let solo = format!("SOLO-{}", rnd());
  create_coupon(&am.client, json!({ "code": c10, "percentOff": 10, "stackable": true })).await;
  create_coupon(&am.client, json!({ "code": c20, "percentOff": 20, "stackable": true })).await;
  create_coupon(&am.client, json!({ "code": solo, "percentOff": 50 })).await; // not stackable

  let m = user_mock(&db).await;
  // Two stackable coupons → 10% + 20% = 30% of 1000 = 300.
  let r = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCodes": [c10, c20] }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::CREATED);
  assert_eq!(
    r.json::<Value>().await.unwrap()["discountAmount"].as_i64().unwrap(),
    300,
    "stacked discounts sum"
  );

  // Mixing a non-stackable coupon is rejected.
  let r = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCodes": [c10, solo] }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "non-stackable coupon cannot be combined");
}

// ---------------------------------------------------------------------------
// Bulk grant issues to many owners and reports outcomes (idempotent on retry).
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_bulk_grant() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let code = format!("BULK-{}", rnd());
  let cid = create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "requiresGrant": true }))
    .await["id"]
    .as_i64()
    .unwrap();

  let a = TestUser::create(&db).await.unwrap();
  let b = TestUser::create(&db).await.unwrap();
  let owners = json!({ "owners": [
    { "ownerKind": "personal", "ownerId": a.model.id },
    { "ownerKind": "personal", "ownerId": b.model.id },
  ] });

  let r: Value = am
    .client
    .post(&format!("/coupons/{cid}/grant-bulk"))
    .json(&owners)
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(r["granted"].as_u64().unwrap(), 2);

  // Re-running is idempotent: both already issued → skipped.
  let r2: Value = am
    .client
    .post(&format!("/coupons/{cid}/grant-bulk"))
    .json(&owners)
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(r2["skipped"].as_u64().unwrap(), 2, "already-issued owners are skipped");
  assert_eq!(r2["granted"].as_u64().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// A failed payment reverses the coupon: the consumed grant is returned.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_failed_payment_reverses_coupon_grant() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if infra::settings::get().membership.payment.provider != "mock" {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;
  let code = format!("FR-{}", rnd());
  let cid = create_coupon(&am.client, json!({ "code": code, "percentOff": 10, "requiresGrant": true }))
    .await["id"]
    .as_i64()
    .unwrap();

  let user = TestUser::create(&db).await.unwrap();
  let m = Mock::new().await;
  user.login(&m.client).await;
  am.client
    .post(&format!("/coupons/{cid}/grant"))
    .json(&json!({ "ownerId": user.model.id }))
    .send()
    .await
    .unwrap();

  async fn mine_has(m: &Mock, code: &str) -> bool {
    let v: Value = m.client.get("/coupons/mine").send().await.unwrap().json().await.unwrap();
    v["items"].as_array().unwrap().iter().any(|i| i["code"].as_str() == Some(code))
  }
  assert!(mine_has(&m, &code).await, "coupon is in the user's wallet before use");

  // Checkout consumes the grant, then the payment FAILS → the grant is restored.
  let co: Value = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "subscription", "planCode": plan, "couponCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let pc = co["purchaseCode"].as_str().unwrap();
  assert!(!mine_has(&m, &code).await, "grant consumed during checkout");

  infra::payment::mock::script(pc, "mock:failed");
  let cf = m
    .client
    .post("/payments/confirm")
    .json(&json!({ "purchaseCode": pc, "paymentId": pc }))
    .send()
    .await
    .unwrap();
  assert_eq!(cf.status(), StatusCode::PAYMENT_REQUIRED, "payment failed");
  assert!(mine_has(&m, &code).await, "failed payment returned the coupon to the wallet");
}

// ---------------------------------------------------------------------------
// Coupon management is gated on its own namespace. A caller without the gate is
// refused by the route, not by the account — the two are different refusals and
// collapsing them would let the row scope be deleted with the suite still green.
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
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "a caller without the coupon gate cannot create one");
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

// ---------------------------------------------------------------------------
// Free-subscribe path (POST /subscriptions) also applies discount coupons. It takes no payment, so a
// coupon that leaves money owed is refused there (the plan goes through checkout), while one that
// covers the whole first period starts it, recording the discounted purchase.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_free_subscribe_applies_coupon() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;

  let partial = format!("SUBPCT-{}", rnd());
  create_coupon(&am.client, json!({ "code": partial, "percentOff": 25 })).await;
  let whole = format!("SUBFREE-{}", rnd());
  create_coupon(&am.client, json!({ "code": whole, "percentOff": 100 })).await;
  let plan = create_sub_plan(&am.client, 1000).await; // paid first period (no trial)

  // a user subscribes with the coupon (no PG checkout)
  let m = Mock::new().await;
  let user = TestUser::create(&db).await.unwrap();
  user.login(&m.client).await;
  let resp = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": plan, "couponCode": partial }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED, "750 still owed and nothing paid");

  let resp = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": plan, "couponCode": whole }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);

  // the recorded purchase reflects the discount: subtotal 1000, amount 0
  use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
  let purchase = entity::prelude::MembershipPurchases::find()
    .filter(entity::membership::membership_purchases::Column::OwnerId.eq(user.model.id))
    .order_by_desc(entity::membership::membership_purchases::Column::Id)
    .one(&db)
    .await
    .unwrap()
    .expect("purchase recorded");
  assert_eq!(purchase.subtotal, 1000);
  assert_eq!(purchase.amount, 0, "100% off 1000");
}

// ---------------------------------------------------------------------------
// The descriptor of a list whose only gate is the session guard.
// ---------------------------------------------------------------------------

/// Coupons, purchases and subscriptions gate their list with the session guard alone, so their
/// descriptor is mounted with no gate of its own and merged into the branch that guard covers.
/// It has to be reachable exactly where the list is, and refused exactly where the list is.
#[tokio::test]
async fn test_session_gated_descriptor_stands_where_its_list_stands() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let anon = Mock::new().await;

  for path in ["/coupons", "/membership-purchases", "/subscriptions"] {
    let list = anon.client.get(path).send().await.unwrap().status();
    let descriptor = anon.client.get(&format!("{path}/filters")).send().await.unwrap().status();

    assert_ne!(list, StatusCode::OK, "{path} must refuse an unauthenticated caller");
    assert_eq!(list, descriptor, "{path}/filters must refuse exactly where the list refuses");

    let resp = am.client.get(&format!("{path}/filters")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{path}/filters is served to a caller who may list");

    let body: Value = resp.json().await.unwrap();
    assert!(body["defaultSort"].as_str().is_some(), "{path}: {body}");
    assert!(!body["fields"].as_array().unwrap().is_empty(), "{path}: {body}");
    assert!(body["limits"]["maxConditionNodes"].as_u64().is_some(), "{path}: {body}");
  }
}

// ---------------------------------------------------------------------------
// Single use holds under concurrency: many checkouts fired at once with one single-use grant, or
// with a once-per-owner coupon, discount exactly one purchase.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_coupon_concurrent_checkouts_redeem_once() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_sub_plan(&am.client, 1000).await;

  let granted = format!("RACE-G-{}", rnd());
  let gid = create_coupon(&am.client, json!({ "code": granted, "percentOff": 50, "requiresGrant": true }))
    .await["id"]
    .as_i64()
    .unwrap();
  let capped = format!("RACE-O-{}", rnd());
  create_coupon(&am.client, json!({ "code": capped, "percentOff": 50, "maxRedemptionsPerOwner": 1 })).await;

  let user = TestUser::create(&db).await.unwrap();
  let m = Mock::new().await;
  user.login(&m.client).await;
  am.client
    .post(&format!("/coupons/{gid}/grant"))
    .json(&json!({ "ownerId": user.model.id }))
    .send()
    .await
    .unwrap();

  for code in [&granted, &capped] {
    let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });
    let attempts = (0..8).map(|_| m.client.post("/payments/checkout").json(&body).send());
    let statuses: Vec<StatusCode> = futures::future::join_all(attempts)
      .await
      .into_iter()
      .map(|r| r.unwrap().status())
      .collect();
    let created = statuses.iter().filter(|s| **s == StatusCode::CREATED).count();
    assert_eq!(
      created, 1,
      "coupon `{code}` discounted exactly one of the racing checkouts: {statuses:?}"
    );
    assert!(
      statuses.iter().all(|s| *s == StatusCode::CREATED || *s == StatusCode::BAD_REQUEST),
      "the losers are refused, not failed: {statuses:?}"
    );
  }
}
