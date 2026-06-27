pub mod coupons;
pub mod credit_grants;
pub mod credit_reservations;
pub mod credit_transactions;
pub mod credit_wallets;
pub mod membership_plans;
pub mod membership_purchases;
pub mod payment_webhook_events;
pub mod plan_actions;
pub mod subscription_events;
pub mod subscription_usage;
pub mod subscriptions;

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Who owns a membership / wallet: an individual user or an organization.
/// Also used as a plan's `scope` (which owner kind may subscribe).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum OwnerKind {
  Personal,
  Organization,
}

/// Billing model of a plan.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum PlanKind {
  Subscription,
  Credit,
}

/// UX-facing label for a subscription plan's billing cadence.
/// `duration_days` is the authoritative length; this is presentation only.
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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum PurchaseStatus {
  Pending,
  Completed,
  Failed,
  Refunded,
}

/// Kind of credit ledger entry.
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
}

/// Lifecycle of a credit reservation (authorize → capture/release).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum ReservationStatus {
  Held,
  Captured,
  Released,
}
