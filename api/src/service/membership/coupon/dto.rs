use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::OwnerKind;
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponFilter {
  pub is_active: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCoupon {
  pub code: String,
  /// Percentage discount (0–100). Mutually exclusive with `amount_off`.
  #[serde(default)]
  pub percent_off: Option<i32>,
  /// Fixed discount in minor units of `currency`. Mutually exclusive with `percent_off`.
  #[serde(default)]
  pub amount_off: Option<i64>,
  #[serde(default)]
  pub currency: Option<String>,
  #[serde(default)]
  pub max_redemptions: Option<i32>,
  /// Per-owner cap (e.g. 1 = once per customer).
  #[serde(default)]
  pub max_redemptions_per_owner: Option<i32>,
  /// When true, only owners the coupon is granted to may redeem it.
  #[serde(default)]
  pub requires_grant: bool,
  /// When true, combinable with other stackable coupons in one checkout.
  #[serde(default)]
  pub stackable: bool,
  /// Restrict the coupon to these plan codes (omit/empty = all plans).
  #[serde(default)]
  pub plan_codes: Option<Vec<String>>,
  #[serde(default)]
  pub expires_at: Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct CouponItem {
  pub id: i32,
  pub code: String,
  pub percent_off: Option<i32>,
  pub amount_off: Option<i64>,
  pub currency: Option<String>,
  pub max_redemptions: Option<i32>,
  pub max_redemptions_per_owner: Option<i32>,
  pub redeemed_count: i32,
  pub requires_grant: bool,
  pub stackable: bool,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub is_active: bool,
  pub created_at: DateTimeWithTimeZone,
}

/// Issue a coupon to a specific owner.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantCoupon {
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
}

/// Issue a coupon to many owners at once.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkGrantCoupon {
  pub owners: Vec<GrantCoupon>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkGrantResult {
  /// Newly issued or re-issued.
  pub granted: usize,
  /// Already held an issued grant (idempotent no-op).
  pub skipped: usize,
  /// Already redeemed — not re-issued (revoke first).
  pub conflicts: usize,
}

impl IntoResponse for BulkGrantResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

/// Result of applying one coupon during checkout.
pub struct AppliedCoupon {
  pub code: String,
  pub discount: i64,
  pub stackable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponGranted;

impl IntoResponse for CouponGranted {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(serde_json::json!({}))).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponRevoked;

impl IntoResponse for CouponRevoked {
  fn into_response(self) -> axum::response::Response {
    Json(serde_json::json!({})).into_response()
  }
}

/// A coupon grant with its lifecycle timestamps (admin analytics). Status is
/// `revoked` if `revokedAt` is set, else `redeemed` if `redeemedAt` is set, else
/// `issued`.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct CouponGrantItem {
  pub id: i32,
  pub coupon_id: i32,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub redeemed_at: Option<DateTimeWithTimeZone>,
  pub revoked_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
}

/// A coupon available to the calling owner ("my coupons").
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct MyCouponItem {
  pub id: i32,
  pub code: String,
  pub percent_off: Option<i32>,
  pub amount_off: Option<i64>,
  pub currency: Option<String>,
  pub expires_at: Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponCreated {
  pub id: i32,
  pub code: String,
}

impl IntoResponse for CouponCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

/// Partial update; only present fields are changed. Use `is_active=false` to retire
/// a coupon without deleting it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCoupon {
  #[serde(default)]
  pub is_active: Option<bool>,
  #[serde(default)]
  pub max_redemptions: Option<i32>,
  #[serde(default)]
  pub max_redemptions_per_owner: Option<i32>,
  #[serde(default)]
  pub requires_grant: Option<bool>,
  #[serde(default)]
  pub stackable: Option<bool>,
  #[serde(default)]
  pub expires_at: Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponUpdated;

impl IntoResponse for CouponUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(serde_json::json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponDeleted;

impl IntoResponse for CouponDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(serde_json::json!({})).into_response()
  }
}
