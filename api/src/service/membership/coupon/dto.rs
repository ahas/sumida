use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

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
  pub redeemed_count: i32,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub is_active: bool,
  pub created_at: DateTimeWithTimeZone,
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
