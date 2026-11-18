pub mod coupon_grants;
pub mod coupon_plans;
pub mod coupon_redemptions;
pub mod coupons;
pub mod credit_grants;
pub mod credit_reservations;
pub mod credit_transactions;
pub mod credit_wallets;
pub mod membership_plans;
pub mod membership_purchases;
pub mod payment_webhook_events;
pub mod plan_actions;
pub mod quota_entries;
pub mod quota_reservations;
pub mod subscription_events;
pub mod subscription_usage;
pub mod subscriptions;

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Who owns a membership / wallet: an individual user or an organization.
/// Also used as a plan's `scope` (which owner kind may subscribe).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum OwnerKind {
  Personal,
  Organization,
}

/// 목록 필터가 값을 나열해 올 때 조각 하나하나를 이 구현이 읽는다. 모르는 값은 여기서 거절되어
/// 400 이 된다 — 조용히 버리면 걸리지 않은 필터가 걸린 것처럼 보인다.
impl std::str::FromStr for OwnerKind {
  type Err = DbErr;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    <Self as sea_orm::ActiveEnum>::try_from_value(&s.to_string())
  }
}

/// Billing model of a plan.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum PlanKind {
  Subscription,
  Credit,
}

/// 목록 필터가 값을 나열해 올 때 조각 하나하나를 이 구현이 읽는다. 모르는 값은 여기서 거절되어
/// 400 이 된다 — 조용히 버리면 걸리지 않은 필터가 걸린 것처럼 보인다.
impl std::str::FromStr for PlanKind {
  type Err = DbErr;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    <Self as sea_orm::ActiveEnum>::try_from_value(&s.to_string())
  }
}

/// UX-facing label for a subscription plan's billing cadence.
/// `duration_days` is the authoritative length; this is presentation only.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BillingInterval {
  Day,
  Week,
  Month,
  Year,
}

/// Lifecycle state of a subscription instance. `current_period_end` is the
/// authoritative entitlement gate; `status` is a denormalization for reporting.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionStatus {
  Pending,
  Active,
  PastDue,
  Canceled,
  Expired,
}

/// 목록 필터가 값을 나열해 올 때 조각 하나하나를 이 구현이 읽는다. 모르는 값은 여기서 거절되어
/// 400 이 된다 — 조용히 버리면 걸리지 않은 필터가 걸린 것처럼 보인다.
impl std::str::FromStr for SubscriptionStatus {
  type Err = DbErr;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    <Self as sea_orm::ActiveEnum>::try_from_value(&s.to_string())
  }
}

/// Append-only audit events for subscription state transitions.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionEventKind {
  Created,
  Renewed,
  Canceled,
  Reactivated,
  Expired,
  StatusChanged,
}

/// Status of a purchase (canonical record of a payment event).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum PurchaseStatus {
  Pending,
  Completed,
  Failed,
  Refunded,
}

/// 목록 필터가 값을 나열해 올 때 조각 하나하나를 이 구현이 읽는다. 모르는 값은 여기서 거절되어
/// 400 이 된다 — 조용히 버리면 걸리지 않은 필터가 걸린 것처럼 보인다.
impl std::str::FromStr for PurchaseStatus {
  type Err = DbErr;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    <Self as sea_orm::ActiveEnum>::try_from_value(&s.to_string())
  }
}

/// Kind of credit ledger entry.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum CreditTxKind {
  Grant,
  Consume,
  Refund,
  Expire,
  Adjust,
  /// A soft hold placed by `reserve` (no balance change; amount 0).
  Reserve,
  /// A reservation confirmed into a real spend (debits lots; amount negative).
  Capture,
  /// A reservation cancelled without spending (no balance change; amount 0).
  Release,
  /// A reservation grown by a further amount (no balance change; amount 0).
  Extend,
  /// Debt paid off from free balance (amount and debt change both negative).
  Repay,
}

/// Kind of amount-quota ledger entry.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum QuotaEntryKind {
  /// An amount spent straight away through the entitlement gate.
  Consume,
  /// A hold placed.
  Reserve,
  /// A hold grown.
  Extend,
  /// A hold turned into usage at the actual amount.
  Capture,
  /// A hold dropped, by its caller or by its expiry.
  Release,
}

/// Lifecycle of a credit reservation (authorize → capture/release).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum ReservationStatus {
  Held,
  Captured,
  Released,
}
