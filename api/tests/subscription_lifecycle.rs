//! Subscription lifecycle extras: reactivation, trial/grace, dunning.
//! Each assertion targets the subscription the test created (shared test DB).

use api::util::random::generate_url_safe_string;
use e2e::prelude::*;
use entity::membership::subscription_events::Column as EventCol;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
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
