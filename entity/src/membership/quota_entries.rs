use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::QuotaEntryKind;

/// Append-only ledger of a subscription's amount-quota steps. `amount_change` is the
/// signed change to the period's `amount_used` and `held_change` the signed change to
/// what its holds take out of the room. `unique(subscription_id, idempotency_key)`
/// makes every step idempotent per subscription.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "quota_entries")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:              i64,
  #[sea_orm(indexed)]
  pub subscription_id: i32,
  pub action:          String,
  pub period_start:    DateTimeUtc,
  pub kind:            QuotaEntryKind,
  #[sea_orm(default_value = 0)]
  pub amount_change:   i64,
  #[sea_orm(default_value = 0)]
  pub held_change:     i64,
  #[sea_orm(nullable)]
  pub reservation_id:  Option<i32>,
  #[sea_orm(nullable)]
  pub reference_code:  Option<String>,
  #[sea_orm(nullable)]
  pub idempotency_key: Option<String>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:      DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::subscriptions::Entity",
    from = "Column::SubscriptionId",
    to = "super::subscriptions::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Subscription,
}

impl Related<super::subscriptions::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Subscription.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
