use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::{OwnerKind, PlanKind};
use serde::{Deserialize, Serialize};

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

/// Start a PG-backed purchase. Creates a pending purchase and returns the order
/// id to register with the PG (PortOne `paymentId`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutRequest {
  pub kind: PlanKind,
  pub plan_code: String,
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  /// Optional discount coupon code (single).
  #[serde(default)]
  pub coupon_code: Option<String>,
  /// Optional multiple coupon codes (stacking — all must be `stackable`).
  #[serde(default)]
  pub coupon_codes: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutResult {
  /// Merchant order id = `membership_purchases.code`; use as the PG `paymentId`.
  pub purchase_code: String,
  /// Total to charge (= subtotal - discount + tax).
  pub amount: i64,
  pub subtotal: i64,
  pub discount_amount: i64,
  pub tax_amount: i64,
  pub currency: String,
  pub provider: String,
}

impl IntoResponse for CheckoutResult {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

/// Confirm a purchase after the client has paid (server-side verification).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmRequest {
  pub purchase_code: String,
  /// PG payment id. For PortOne V2 this equals `purchase_code`.
  pub payment_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmResult {
  pub purchase_code: String,
  pub status: String,
  pub kind: PlanKind,
  pub subscription_id: Option<i32>,
  pub wallet_id: Option<i32>,
}

impl IntoResponse for ConfirmResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct WebhookAck {
  pub received: bool,
}

impl IntoResponse for WebhookAck {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRefundRequest {
  pub purchase_code: String,
  #[serde(default)]
  pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRefundResult {
  pub purchase_code: String,
  pub status: String,
  pub cancelled_amount: i64,
}

impl IntoResponse for PaymentRefundResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}
