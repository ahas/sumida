use axum::Json;
use axum::response::IntoResponse;
use entity::membership::{OwnerKind, ReservationStatus};
use sea_orm::prelude::DateTimeUtc;
use serde::{Deserialize, Serialize};

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

/// Hold an amount of the quota the owner's live subscription carries for an action.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveQuota {
  pub action:          String,
  /// What to hold; positive.
  pub amount:          i64,
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

/// Turn a quota hold into usage at the amount actually used.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureQuota {
  /// What was actually used; may be more than the hold. Left out, the hold's amount.
  #[serde(default)]
  pub amount:          Option<i64>,
  /// How far past the period's limit the usage may go rather than be refused. Left out, 0.
  #[serde(default)]
  pub overage_limit:   Option<i64>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

/// A quota hold as it stands after a step, beside the period it holds against.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaReservationResult {
  pub reservation_id:  i32,
  pub subscription_id: i32,
  pub action:          String,
  /// What the hold holds; once captured, what was recorded as usage.
  pub amount:          i64,
  pub status:          ReservationStatus,
  pub expires_at:      DateTimeUtc,
  pub period_start:    DateTimeUtc,
  /// The period's amount limit; `null` = unlimited.
  pub limit:           Option<i64>,
  /// What the period has used.
  pub amount_used:     i64,
  /// What live holds take out of the period's room.
  pub held:            i64,
  /// `limit - amountUsed - held`, never below zero; `null` = unlimited.
  pub remaining:       Option<i64>,
}

impl IntoResponse for QuotaReservationResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}
