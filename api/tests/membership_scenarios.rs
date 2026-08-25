//! End-to-end membership journeys exercising many features together.

use api::service::membership::maintenance;
use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::ap_test_utils::grant_org_scope;
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use serde_json::{Value, json};

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

fn is_mock() -> bool {
  infra::settings::get().membership.payment.provider == "mock"
}

async fn admin_mock(db: &sea_orm::DatabaseConnection) -> Mock {
  let m = Mock::new().await;
  TestUser::create_admin(db).await.unwrap().login(&m.client).await;
  m
}

async fn user_login(db: &sea_orm::DatabaseConnection) -> (Mock, TestUser) {
  let m = Mock::new().await;
  let u = TestUser::create(db).await.unwrap();
  u.login(&m.client).await;
  (m, u)
}

async fn create_plan(client: &TestClient, body: Value) -> String {
  let r = client.post("/membership-plans").json(&body).send().await.unwrap();
  assert_eq!(r.status(), StatusCode::CREATED, "create plan");
  r.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string()
}

async fn check(client: &TestClient, action: &str) -> reqwest::Response {
  client
    .post("/entitlements/check")
    .json(&json!({ "action": action }))
    .send()
    .await
    .unwrap()
}

// ===========================================================================
// 1) SaaS customer lifecycle: trial → quota use → credit fallback → auto-renew
//    (quota reset) → mid-period upgrade with proration.
// ===========================================================================
#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn scenario_saas_customer_lifecycle() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  if !is_mock() {
    eprintln!("skipping: payment provider is not 'mock'");
    return;
  }
  let am = admin_mock(&db).await;
  // Unique action — the shared test DB has other "render" credit plans.
  let action = format!("render-{}", rnd());

  let pro = create_plan(
    &am.client,
    json!({
      "code": format!("pro-{}", rnd()), "name": "Pro", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "trialDays": 7,
      "actions": [{ "action": &action, "quotaPerPeriod": 2 }],
    }),
  )
  .await;
  let credits = create_plan(
    &am.client,
    json!({
      "code": format!("cr-{}", rnd()), "name": "Credits", "kind": "credit", "scope": "personal",
      "price": 5000, "creditAmount": 100,
      "actions": [{ "action": &action, "creditCost": 10 }],
    }),
  )
  .await;

  // Subscribe to Pro on a free trial with auto-renew.
  let (m, _u) = user_login(&db).await;
  let created: Value = m
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": pro, "autoRenew": true, "billingKey": "mock:bill:ok" }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let sub_id = created["id"].as_i64().unwrap() as i32;
  let sub = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap();
  assert!(sub.trial_ends_at.is_some(), "started on a trial");
  assert!(sub.current_period_end < Utc::now() + Duration::days(8), "trial period ~7 days");

  // Use the quota (2/period) via the subscription; the third use is over quota and
  // the wallet is empty → 402 (the action is credit-coverable but unpaid).
  for _ in 0..2 {
    let r = check(&m.client, &action).await;
    assert_eq!(r.json::<Value>().await.unwrap()["via"].as_str().unwrap(), "subscription");
  }
  assert_eq!(
    check(&m.client, &action).await.status(),
    StatusCode::PAYMENT_REQUIRED,
    "quota exhausted, credits coverable but empty"
  );

  // Buy credits (PG checkout → confirm) and the next render falls through to them.
  let co: Value = m
    .client
    .post("/payments/checkout")
    .json(&json!({ "kind": "credit", "planCode": credits }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let pc = co["purchaseCode"].as_str().unwrap();
  let cf = m
    .client
    .post("/payments/confirm")
    .json(&json!({ "purchaseCode": pc, "paymentId": pc }))
    .send()
    .await
    .unwrap();
  assert_eq!(cf.status(), StatusCode::OK, "credit purchase confirmed");

  let r = check(&m.client, &action).await;
  let out: Value = r.json().await.unwrap();
  assert_eq!(out["via"].as_str().unwrap(), "credit", "quota spent → credit path");
  assert_eq!(out["consumed"].as_i64().unwrap(), 10);
  let bal: Value = m.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 90);

  // Trial ends → auto-renew charges and starts a fresh period (quota resets).
  let mut a = Subscriptions::find_by_id(sub_id).one(&db).await.unwrap().unwrap().into_active_model();
  a.current_period_end = Set(Utc::now() - Duration::hours(1));
  a.update(&db).await.unwrap();
  let (renewed, _) = maintenance::renew_due_subscriptions(&db).await.unwrap();
  assert!(renewed >= 1, "auto-renew charged the billing key");
  assert_eq!(
    check(&m.client, &action).await.json::<Value>().await.unwrap()["via"].as_str().unwrap(),
    "subscription",
    "renewal reset the per-period quota"
  );

  // Upgrade mid-period: positive proration is charged via the billing key.
  let pro_plus = create_plan(
    &am.client,
    json!({
      "code": format!("prop-{}", rnd()), "name": "Pro+", "kind": "subscription", "scope": "personal",
      "price": 3000, "durationDays": 30, "actions": [{ "action": &action, "quotaPerPeriod": 10 }],
    }),
  )
  .await;
  let pr = m
    .client
    .post(&format!("/subscriptions/{sub_id}/change-plan"))
    .json(&json!({ "planCode": pro_plus }))
    .send()
    .await
    .unwrap();
  assert_eq!(pr.status(), StatusCode::OK);
  assert!(
    pr.json::<Value>().await.unwrap()["prorationAmount"].as_i64().unwrap() > 0,
    "upgrade prorated"
  );
  let got: Value = m
    .client
    .get(&format!("/subscriptions/{sub_id}"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(got["planCode"].as_str().unwrap(), pro_plus, "now on the upgraded plan");
}

// ===========================================================================
// 2) Organization admin journey: a parent-org manager subscribes a CHILD org
//    and funds its credits (hierarchy delegation); a child member spends them;
//    the manager sees the child's subscription in their list.
// ===========================================================================
#[tokio::test]
async fn scenario_org_admin_hierarchy() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;

  let team = create_plan(
    &am.client,
    json!({
      "code": format!("team-{}", rnd()), "name": "Team", "kind": "subscription", "scope": "organization",
      "durationDays": 30, "actions": [{ "action": "team-feature" }],
    }),
  )
  .await;
  let org_credits = create_plan(
    &am.client,
    json!({
      "code": format!("oc-{}", rnd()), "name": "OrgCredits", "kind": "credit", "scope": "organization",
      "actions": [{ "action": "gpu", "creditCost": 5 }],
    }),
  )
  .await;

  // Parent org with a child org under it.
  let parent = TestOrg::create(&db).await.unwrap();
  let child_id = Organizations::insert(entity::organizations::ActiveModel {
    name: Set(format!("child-{}", rnd())),
    parent_id: Set(Some(parent.model.id)),
    ..Default::default()
  })
  .exec(&db)
  .await
  .unwrap()
  .last_insert_id;

  // Manager in the parent org, with org policies on the PARENT only.
  let manager = TestUser::create_in_org(&db, parent.model.id).await.unwrap();
  // 상위 조직의 구독·크레딧 관리 층. 상위 조직에 붙은 부여가 하위 조직까지 닿는 것은 `^`
  // 닫힘이 맡는다.
  grant_org_scope(&db, &manager, parent.model.id, "subscription_manager").await.unwrap();
  grant_org_scope(&db, &manager, parent.model.id, "credit_manager").await.unwrap();
  let mm = Mock::new().await;
  manager.login(&mm.client).await;

  // Hierarchy delegation: the parent manager subscribes the CHILD org.
  let sub = mm
    .client
    .post("/subscriptions")
    .json(&json!({ "planCode": team, "ownerKind": "organization", "ownerId": child_id }))
    .send()
    .await
    .unwrap();
  assert_eq!(sub.status(), StatusCode::CREATED, "parent manager subscribes the child org");

  // …and funds the child org's credits.
  let grant = mm
    .client
    .post("/credits/grant")
    .json(
      &json!({ "ownerKind": "organization", "ownerId": child_id, "amount": 50, "planCode": org_credits }),
    )
    .send()
    .await
    .unwrap();
  assert_eq!(grant.status(), StatusCode::OK);

  // A member of the child org spends them.
  let member = TestUser::create_in_org(&db, child_id).await.unwrap();
  let mc = Mock::new().await;
  member.login(&mc.client).await;
  let consume = mc
    .client
    .post("/credits/consume")
    .json(&json!({ "action": "gpu", "ownerKind": "organization", "ownerId": child_id }))
    .send()
    .await
    .unwrap();
  assert_eq!(consume.status(), StatusCode::OK, "child member spends the org credits");

  // The manager's subscription list surfaces the child org's subscription.
  let list: Value = mm.client.get("/subscriptions").send().await.unwrap().json().await.unwrap();
  let sees_child = list["items"].as_array().unwrap().iter().any(|i| {
    i["ownerKind"].as_str() == Some("organization") && i["ownerId"].as_i64() == Some(child_id as i64)
  });
  assert!(sees_child, "parent manager sees the child org subscription (list hierarchy scoping)");
}

// ===========================================================================
// 3) Marketing campaign: bulk-issue an exclusive coupon, one recipient redeems,
//    one is recalled before use, then analytics reflect issued/redeemed/revoked.
// ===========================================================================
#[tokio::test]
async fn scenario_marketing_campaign() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let am = admin_mock(&db).await;
  let plan = create_plan(
    &am.client,
    json!({
      "code": format!("camp-{}", rnd()), "name": "Camp", "kind": "subscription", "scope": "personal",
      "price": 1000, "durationDays": 30, "actions": [{ "action": "a" }],
    }),
  )
  .await;
  let code = format!("VIP30-{}", rnd());
  let cid = am
    .client
    .post("/coupons")
    .json(&json!({ "code": code, "percentOff": 30, "requiresGrant": true, "maxRedemptions": 100 }))
    .send()
    .await
    .unwrap()
    .json::<Value>()
    .await
    .unwrap()["id"]
    .as_i64()
    .unwrap();

  // Three targeted recipients.
  let (ma, a) = user_login(&db).await;
  let (mb, b) = user_login(&db).await;
  let (mc, c) = user_login(&db).await;
  let bulk: Value = am
    .client
    .post(&format!("/coupons/{cid}/grant-bulk"))
    .json(&json!({ "owners": [
      { "ownerId": a.model.id }, { "ownerId": b.model.id }, { "ownerId": c.model.id },
    ] }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(bulk["granted"].as_u64().unwrap(), 3, "issued to all three");

  let buy = |m: &Mock| {
    let body = json!({ "kind": "subscription", "planCode": plan, "couponCode": code });
    m.client.post("/payments/checkout").json(&body)
  };

  // A redeems (discount applied, grant consumed).
  let ra = buy(&ma).send().await.unwrap();
  assert_eq!(ra.status(), StatusCode::CREATED);
  assert_eq!(ra.json::<Value>().await.unwrap()["discountAmount"].as_i64().unwrap(), 300);

  // B is recalled before using it → B can no longer redeem.
  let rv = am
    .client
    .delete(&format!("/coupons/{cid}/grant?ownerId={}", b.model.id))
    .send()
    .await
    .unwrap();
  assert_eq!(rv.status(), StatusCode::OK);
  assert_eq!(
    buy(&mb).send().await.unwrap().status(),
    StatusCode::BAD_REQUEST,
    "revoked recipient cannot redeem"
  );

  // C redeems normally.
  assert_eq!(buy(&mc).send().await.unwrap().status(), StatusCode::CREATED);

  // Analytics: 3 grants, ≥2 redeemed (A, C), ≥1 revoked (B).
  let grants: Value = am
    .client
    .get(&format!("/coupons/{cid}/grants"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(grants["count"].as_u64().unwrap(), 3);
  let items = grants["items"].as_array().unwrap();
  let redeemed = items.iter().filter(|i| !i["redeemedAt"].is_null()).count();
  let revoked = items.iter().filter(|i| !i["revokedAt"].is_null()).count();
  assert!(redeemed >= 2, "two recipients redeemed");
  assert!(revoked >= 1, "one recipient was recalled");
}
