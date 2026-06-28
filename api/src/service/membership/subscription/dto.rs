use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::{OwnerKind, SubscriptionStatus};
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionFilter {
  pub owner_kind: Option<OwnerKind>,
  pub owner_id: Option<i32>,
  pub status: Option<SubscriptionStatus>,
  pub plan_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListItem {
  pub id: i32,
  pub code: String,
  pub plan_id: i32,
  pub plan_code: String,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub status: SubscriptionStatus,
  pub current_period_start: DateTimeWithTimeZone,
  pub current_period_end: DateTimeWithTimeZone,
  pub cancel_at_period_end: bool,
  pub auto_renew: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetSubscription {
  pub id: i32,
  pub code: String,
  pub plan_id: i32,
  pub plan_code: String,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub status: SubscriptionStatus,
  pub current_period_start: DateTimeWithTimeZone,
  pub current_period_end: DateTimeWithTimeZone,
  pub cancel_at_period_end: bool,
  pub canceled_at: Option<DateTimeWithTimeZone>,
  pub auto_renew: bool,
  pub allowed_actions: JsonValue,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetSubscription {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscription {
  pub plan_code: String,
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
  /// Opt the new subscription into auto-renewal (requires `billing_key`).
  #[serde(default)]
  pub auto_renew: bool,
  /// PG billing key (정기결제) stored for auto-renewal.
  #[serde(default)]
  pub billing_key: Option<String>,
  /// Optional discount coupon(s) applied to the first period's recorded amount (parity with the paid
  /// checkout path). Only meaningful when the first period is charged (i.e. not a free trial).
  #[serde(default)]
  pub coupon_code: Option<String>,
  #[serde(default)]
  pub coupon_codes: Option<Vec<String>>,
}

/// Toggle auto-renew on an existing subscription. Enabling requires a billing key
/// (either supplied here or already stored).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutoRenew {
  pub enabled: bool,
  #[serde(default)]
  pub billing_key: Option<String>,
}

/// Switch an active subscription to a different plan mid-period (upgrade/downgrade).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
  pub plan_code: String,
}

/// Proration breakdown for a mid-period plan change. Amounts are in minor currency
/// units, prorated by the fraction of the current period still remaining. Actual
/// money movement (charging `proration_amount` or crediting a negative one) is the
/// PG layer's responsibility; this records the canonical breakdown.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProrationResult {
  pub subscription_id: i32,
  pub old_plan_code: String,
  pub new_plan_code: String,
  /// Unused remainder of the old plan's price for this period (a credit).
  pub old_unused: i64,
  /// New plan's price prorated for the remaining period (a charge).
  pub new_prorated: i64,
  /// Net to settle now = new_prorated - old_unused (negative = credit owed).
  pub proration_amount: i64,
  /// Tracking code of the recorded proration purchase.
  pub purchase_code: String,
}

impl IntoResponse for ProrationResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelQuery {
  #[serde(default)]
  pub at_period_end: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCreated {
  pub id: i32,
  pub code: String,
}

impl IntoResponse for SubscriptionCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct SubscriptionUpdated;

impl IntoResponse for SubscriptionUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
