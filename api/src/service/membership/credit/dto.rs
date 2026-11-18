use axum::Json;
use axum::response::IntoResponse;
use entity::membership::{CreditTxKind, OwnerKind};
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantCredit {
  #[serde(default = "default_owner_kind")]
  pub owner_kind:      OwnerKind,
  #[serde(default)]
  pub owner_id:        Option<i32>,
  pub amount:          i64,
  /// Optional source plan code recorded on the lot (for audit).
  #[serde(default)]
  pub plan_code:       Option<String>,
  #[serde(default)]
  pub expiry_at:       Option<DateTimeUtc>,
  #[serde(default)]
  pub reference_code:  Option<String>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumeCredit {
  /// The action spent on. Without `amount`, its fixed cost under a credit plan is what is spent;
  /// beside `amount`, it only labels the ledger entry.
  #[serde(default)]
  pub action:          Option<String>,
  /// What to spend. Left out, the action's fixed cost.
  #[serde(default)]
  pub amount:          Option<i64>,
  #[serde(default)]
  pub plan_code:       Option<String>,
  #[serde(default = "default_owner_kind")]
  pub owner_kind:      OwnerKind,
  #[serde(default)]
  pub owner_id:        Option<i32>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerQuery {
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id:   Option<i32>,
}

impl Default for OwnerQuery {
  fn default() -> Self {
    Self {
      owner_kind: OwnerKind::Personal,
      owner_id:   None,
    }
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundCredit {
  #[serde(default = "default_owner_kind")]
  pub owner_kind:      OwnerKind,
  #[serde(default)]
  pub owner_id:        Option<i32>,
  pub amount:          i64,
  #[serde(default)]
  pub reference_code:  Option<String>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletBalance {
  pub wallet_id:  i32,
  pub owner_kind: OwnerKind,
  pub owner_id:   i32,
  /// Cached sum of non-expired lots.
  pub balance:    i64,
  /// Credits currently on hold by live (unexpired) reservations.
  pub reserved:   i64,
  /// What captures could not cover, repaid before anything else is spendable.
  pub debt:       i64,
  /// Immediately spendable = `balance - reserved - debt`, never below zero.
  pub spendable:  i64,
}

impl IntoResponse for WalletBalance {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantResult {
  pub wallet_id: i32,
  pub granted:   i64,
  /// The balance once the grant has repaid what debt it could.
  pub balance:   i64,
  /// The debt left after the grant.
  pub debt:      i64,
}

impl IntoResponse for GrantResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumeResult {
  pub wallet_id:     i32,
  /// What left the balance.
  pub consumed:      i64,
  /// What the step could not cover and recorded as debt instead.
  pub debt_incurred: i64,
  pub balance:       i64,
  /// The wallet's whole debt after the step.
  pub debt:          i64,
}

impl IntoResponse for ConsumeResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveCredit {
  /// The action held for. Without `amount`, its fixed cost is the hold; beside `amount`, it only
  /// labels the hold.
  #[serde(default)]
  pub action:          Option<String>,
  /// What to hold. Left out, the action's fixed cost.
  #[serde(default)]
  pub amount:          Option<i64>,
  #[serde(default)]
  pub plan_code:       Option<String>,
  #[serde(default = "default_owner_kind")]
  pub owner_kind:      OwnerKind,
  #[serde(default)]
  pub owner_id:        Option<i32>,
  #[serde(default)]
  pub reference_code:  Option<String>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
  /// Seconds until the hold stops counting and the scheduler releases it. Left out, the
  /// configured `reservation-ttl-secs`.
  #[serde(default)]
  pub expires_in_secs: Option<i64>,
}

/// Grow a hold that is still live.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendReservation {
  /// What to add to the hold; positive.
  pub amount:          i64,
  #[serde(default)]
  pub idempotency_key: Option<String>,
  /// Seconds from now until the grown hold expires. Left out, the configured lifetime.
  #[serde(default)]
  pub expires_in_secs: Option<i64>,
}

/// Spend a wallet hold at the amount actually used.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureReservation {
  /// What was actually used; may be more than the hold. Left out, the hold's amount.
  #[serde(default)]
  pub amount:          Option<i64>,
  /// How much of a shortfall may be recorded as debt rather than refused. Left out, 0.
  #[serde(default)]
  pub debt_limit:      Option<i64>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

/// Drop a hold without spending it.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReservation {
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationResult {
  pub reservation_id: i32,
  pub wallet_id:      i32,
  pub amount:         i64,
  pub status:         entity::membership::ReservationStatus,
  pub expires_at:     Option<DateTimeUtc>,
  /// Spendable = cached balance minus live holds and debt.
  pub spendable:      i64,
}

impl IntoResponse for ReservationResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationFilter {
  pub wallet_id: Option<i32>,
  pub status:    Option<entity::membership::ReservationStatus>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct ReservationListItem {
  pub id:             i32,
  pub wallet_id:      i32,
  pub amount:         i64,
  pub status:         entity::membership::ReservationStatus,
  pub action:         Option<String>,
  pub reference_code: Option<String>,
  pub expires_at:     Option<DateTimeUtc>,
  pub created_at:     DateTimeUtc,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletAudit {
  pub wallet_id:   i32,
  /// Cached balance (sum of non-expired lots).
  pub balance:     i64,
  /// Sum of all ledger entry amounts (the authoritative running total).
  pub ledger_sum:  i64,
  /// Cached debt.
  pub debt:        i64,
  /// Sum of all ledger entry debt changes (the authoritative debt).
  pub ledger_debt: i64,
  /// True when either cache disagrees with the ledger (a consistency bug).
  pub diverged:    bool,
}

impl IntoResponse for WalletAudit {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditTxFilter {
  pub wallet_id: Option<i32>,
  pub kind:      Option<CreditTxKind>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct CreditTxItem {
  pub id:             i64,
  pub wallet_id:      i32,
  pub kind:           CreditTxKind,
  pub amount:         i64,
  pub debt_change:    i64,
  pub action:         Option<String>,
  pub reference_code: Option<String>,
  pub balance_after:  i64,
  pub created_at:     DateTimeUtc,
}
