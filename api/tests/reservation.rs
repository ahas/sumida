//! Credit reservations — authorize (reserve) → capture / release.
//!
//! Shared test DB: every assertion targets the wallet/reservation the test made.

use api::service::membership::maintenance;
use api::util::random::generate_url_safe_string;
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::prelude::*;
use reqwest::StatusCode;
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use serde_json::{Value, json};

fn rnd() -> String {
  generate_url_safe_string(8).unwrap()
}

/// Admin grants `amount` credits to a fresh user on a fresh credit plan with one
/// action costing `cost`. Returns `(user, action, wallet_id, user_mock)`.
async fn setup_wallet(
  db: &sea_orm::DatabaseConnection,
  amount: i64,
  cost: i64,
) -> (TestUser, String, i64, Mock) {
  let am = Mock::new().await;
  let admin = TestUser::create_admin(db).await.unwrap();
  admin.login(&am.client).await;

  let action = format!("job-{}", rnd());
  let resp = am
    .client
    .post("/membership-plans")
    .json(&json!({
      "code": format!("res-{}", rnd()), "name": "Res", "kind": "credit", "scope": "personal",
      "actions": [{ "action": &action, "creditCost": cost }],
    }))
    .send()
    .await
    .unwrap();
  assert_eq!(resp.status(), StatusCode::CREATED);
  let code = resp.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string();

  let user = TestUser::create(db).await.unwrap();
  let grant: Value = am
    .client
    .post("/credits/grant")
    .json(&json!({ "ownerKind": "personal", "ownerId": user.model.id, "amount": amount, "planCode": code }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let wallet_id = grant["walletId"].as_i64().unwrap();

  let user_mock = Mock::new().await;
  user.login(&user_mock.client).await;
  (user, action, wallet_id, user_mock)
}

// ---------------------------------------------------------------------------
// reserve holds spendable without touching balance; capture debits the lots.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reserve_then_capture() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let (_user, action, _wallet, m) = setup_wallet(&db, 100, 10).await;

  // Reserve: balance stays 100, spendable drops to 90.
  let res: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(res["amount"].as_i64().unwrap(), 10);
  assert_eq!(res["status"].as_str().unwrap(), "held");
  assert_eq!(res["spendable"].as_i64().unwrap(), 90);
  let reservation_id = res["reservationId"].as_i64().unwrap();

  let bal: Value = m.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 100, "reserve must not change lot balance");
  assert_eq!(bal["reserved"].as_i64().unwrap(), 10);
  assert_eq!(bal["spendable"].as_i64().unwrap(), 90);

  // Capture: lots are debited, balance 100 → 90.
  let cap: Value = m
    .client
    .post(&format!("/credits/reservations/{reservation_id}/capture"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(cap["balance"].as_i64().unwrap(), 90);

  let bal: Value = m.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["balance"].as_i64().unwrap(), 90);
  assert_eq!(bal["reserved"].as_i64().unwrap(), 0, "captured reservation no longer holds");
  assert_eq!(bal["spendable"].as_i64().unwrap(), 90);
}

// ---------------------------------------------------------------------------
// A held reservation blocks spending beyond spendable; capture then drains it.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reservation_blocks_overspend() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let (_user, action, _wallet, m) = setup_wallet(&db, 20, 10).await;

  // Hold 10 → spendable 10.
  let res: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let reservation_id = res["reservationId"].as_i64().unwrap();

  // One consume (10) fits within spendable.
  let r = m
    .client
    .post("/credits/consume")
    .json(&json!({ "action": &action }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::OK);

  // Now balance 10 but all of it is held → consume is refused (402).
  let r = m
    .client
    .post("/credits/consume")
    .json(&json!({ "action": &action }))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED, "held credits are not spendable by consume");

  // Capturing the hold spends the last 10 → balance 0.
  let cap: Value = m
    .client
    .post(&format!("/credits/reservations/{reservation_id}/capture"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(cap["balance"].as_i64().unwrap(), 0);
}

// ---------------------------------------------------------------------------
// release frees the hold; double-capture / capture-after-release are rejected.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_release_and_invalid_transitions() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let (_user, action, _wallet, m) = setup_wallet(&db, 20, 10).await;

  let res: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let reservation_id = res["reservationId"].as_i64().unwrap();

  // Release → spendable back to full 20.
  let rel: Value = m
    .client
    .post(&format!("/credits/reservations/{reservation_id}/release"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(rel["status"].as_str().unwrap(), "released");
  assert_eq!(rel["spendable"].as_i64().unwrap(), 20);

  // Capturing a released reservation is rejected.
  let r = m
    .client
    .post(&format!("/credits/reservations/{reservation_id}/capture"))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::BAD_REQUEST, "cannot capture a released reservation");

  // Both lots are now freely spendable.
  for _ in 0..2 {
    let r = m
      .client
      .post("/credits/consume")
      .json(&json!({ "action": &action }))
      .send()
      .await
      .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
  }
}

// ---------------------------------------------------------------------------
// Reserve is idempotent per (wallet, key): a replay returns the same hold.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reserve_idempotent() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let (_user, action, _wallet, m) = setup_wallet(&db, 100, 10).await;

  let key = format!("res-{}", rnd());
  let first: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action, "idempotencyKey": key }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let second: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action, "idempotencyKey": key }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert_eq!(first["reservationId"], second["reservationId"], "same key → same reservation");

  // Only one hold of 10 exists → spendable 90, not 80.
  let bal: Value = m.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["reserved"].as_i64().unwrap(), 10, "idempotent reserve must not double-hold");
  assert_eq!(bal["spendable"].as_i64().unwrap(), 90);
}

// ---------------------------------------------------------------------------
// Reservations are listable (owner-scoped), filterable by status.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_reservation_list() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let (_user, action, wallet_id, m) = setup_wallet(&db, 100, 10).await;

  let res: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let reservation_id = res["reservationId"].as_i64().unwrap();

  // Held reservations show up for the owner.
  let held: Value = m
    .client
    .get(&format!("/credits/reservations?walletId={wallet_id}&status=held"))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  assert!(held["count"].as_u64().unwrap() >= 1);
  assert!(
    held["items"]
      .as_array()
      .unwrap()
      .iter()
      .any(|i| i["id"].as_i64() == Some(reservation_id)),
    "the held reservation is listed"
  );

  // An outsider cannot read the wallet's reservations.
  let other = Mock::new().await;
  TestUser::create(&db).await.unwrap().login(&other.client).await;
  let r = other
    .client
    .get(&format!("/credits/reservations?walletId={wallet_id}"))
    .send()
    .await
    .unwrap();
  assert_eq!(r.status(), StatusCode::FORBIDDEN, "cross-owner reservation read is forbidden");
}

// ---------------------------------------------------------------------------
// The maintenance job auto-releases reservations past their expiry.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_expired_reservation_auto_released() {
  bootstrap::init_once().await;
  let db = infra::db::connect().await;
  let (_user, action, _wallet, m) = setup_wallet(&db, 100, 10).await;

  let res: Value = m
    .client
    .post("/credits/reserve")
    .json(&json!({ "action": &action, "expiresInSecs": 3600 }))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
  let reservation_id = res["reservationId"].as_i64().unwrap() as i32;

  // Backdate the expiry into the past.
  let row = CreditReservations::find_by_id(reservation_id).one(&db).await.unwrap().unwrap();
  let mut a = row.into_active_model();
  a.expires_at = Set(Some(Utc::now() - Duration::hours(1)));
  a.update(&db).await.unwrap();

  let released = maintenance::release_expired_reservations(&db).await.unwrap();
  assert!(released >= 1, "the expired hold should be released");

  let after = CreditReservations::find_by_id(reservation_id).one(&db).await.unwrap().unwrap();
  assert_eq!(
    after.status,
    entity::membership::ReservationStatus::Released,
    "expired reservation must be released"
  );

  // Spendable is restored.
  let bal: Value = m.client.get("/credits/balance").send().await.unwrap().json().await.unwrap();
  assert_eq!(bal["reserved"].as_i64().unwrap(), 0);
  assert_eq!(bal["spendable"].as_i64().unwrap(), 100);
}
