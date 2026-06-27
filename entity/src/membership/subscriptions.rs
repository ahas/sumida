use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::{OwnerKind, SubscriptionStatus};

/// A subscription instance owned by a user or organization.
/// `allowed_actions` snapshots the plan's allowed actions at activation so plan
/// edits never apply retroactively.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "subscriptions")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(indexed, unique)]
  pub code: String,
  #[sea_orm(indexed)]
  pub plan_id: i32,
  pub plan_code: String,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub status: SubscriptionStatus,
  pub current_period_start: DateTimeWithTimeZone,
  pub current_period_end: DateTimeWithTimeZone,
  #[sea_orm(default_value = false)]
  pub cancel_at_period_end: bool,
  #[sea_orm(nullable)]
  pub canceled_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(column_type = "JsonBinary")]
  pub allowed_actions: Json,
  #[sea_orm(nullable)]
  pub idempotency_key: Option<String>,
  /// When true, the maintenance renewal job charges `billing_key` at period end
  /// and extends the period; on charge failure the subscription goes `past_due`.
  #[sea_orm(default_value = false)]
  pub auto_renew: bool,
  /// PG billing key (정기결제) used by auto-renew. Required for `auto_renew`.
  #[sea_orm(nullable)]
  pub billing_key: Option<String>,
  /// Number of failed renewal-charge retries in the current dunning cycle.
  /// Reset to 0 on a successful (re)charge.
  #[sea_orm(default_value = 0)]
  pub dunning_attempts: i32,
  // --- reserved for future payment-provider integration ---
  #[sea_orm(nullable)]
  pub provider_subscription_id: Option<String>,
  #[sea_orm(nullable)]
  pub trial_ends_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(nullable)]
  pub grace_ends_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(nullable)]
  pub prev_plan_id: Option<i32>,
  // ---
  /// When an "expiring soon" notice was last sent for the current period.
  /// Cleared on renewal so the next period can notify again.
  #[sea_orm(nullable)]
  pub expiring_notified_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::membership_plans::Entity",
    from = "Column::PlanId",
    to = "super::membership_plans::Column::Id",
    on_update = "Cascade",
    on_delete = "Restrict"
  )]
  Plan,
  #[sea_orm(has_many = "super::subscription_events::Entity")]
  Events,
}

impl Related<super::membership_plans::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Plan.def()
  }
}

impl Related<super::subscription_events::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Events.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
