use axum::Json;
use axum::response::IntoResponse;
use entity::membership::{CreditTxKind, OwnerKind};
use sea_orm::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantCredit {
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  pub amount: i64,
  /// Optional source plan code recorded on the lot (for audit).
  #[serde(default)]
  pub plan_code: Option<String>,
  #[serde(default)]
  pub expiry_at: Option<DateTimeWithTimeZone>,
  #[serde(default)]
  pub reference_code: Option<String>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumeCredit {
  pub action: String,
  #[serde(default)]
  pub plan_code: Option<String>,
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerQuery {
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
}

impl Default for OwnerQuery {
  fn default() -> Self {
    Self {
      owner_kind: OwnerKind::Personal,
      owner_id: None,
    }
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundCredit {
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  pub amount: i64,
  #[serde(default)]
  pub reference_code: Option<String>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletBalance {
  pub wallet_id: i32,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub currency: String,
  /// Cached sum of non-expired lots.
  pub balance: i64,
  /// Credits currently on hold by reservations.
  pub reserved: i64,
  /// Immediately spendable = `balance - reserved`.
  pub spendable: i64,
}

impl IntoResponse for WalletBalance {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantResult {
  pub wallet_id: i32,
  pub granted: i64,
  pub balance: i64,
}

impl IntoResponse for GrantResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumeResult {
  pub wallet_id: i32,
  pub consumed: i64,
  pub balance: i64,
}

impl IntoResponse for ConsumeResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveCredit {
  /// Action whose per-credit cost determines the hold amount.
  pub action: String,
  #[serde(default)]
  pub plan_code: Option<String>,
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  #[serde(default)]
  pub reference_code: Option<String>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
  /// Auto-release the hold after this many seconds if not captured/released.
  #[serde(default)]
  pub expires_in_secs: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationResult {
  pub reservation_id: i32,
  pub wallet_id: i32,
  pub amount: i64,
  pub status: entity::membership::ReservationStatus,
  /// Spendable = cached balance minus all current holds.
  pub spendable: i64,
}

impl IntoResponse for ReservationResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationFilter {
  pub wallet_id: Option<i32>,
  pub status: Option<entity::membership::ReservationStatus>,
}

#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct ReservationListItem {
  pub id: i32,
  pub wallet_id: i32,
  pub amount: i64,
  pub status: entity::membership::ReservationStatus,
  pub action: Option<String>,
  pub reference_code: Option<String>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletAudit {
  pub wallet_id: i32,
  /// Cached balance (sum of non-expired lots).
  pub balance: i64,
  /// Sum of all ledger entry amounts (the authoritative running total).
  pub ledger_sum: i64,
  /// True when the cache and the ledger disagree (a consistency bug).
  pub diverged: bool,
}

impl IntoResponse for WalletAudit {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditTxFilter {
  pub wallet_id: Option<i32>,
  pub kind: Option<CreditTxKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct CreditTxItem {
  pub id: i64,
  pub wallet_id: i32,
  pub kind: CreditTxKind,
  pub amount: i64,
  pub action: Option<String>,
  pub reference_code: Option<String>,
  pub balance_after: i64,
  pub created_at: DateTimeWithTimeZone,
}
