use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-period usage counter for quota-limited subscription actions. Keyed by the
/// subscription's `current_period_start`, so quota resets automatically each
/// period (renewal advances the period start). Only actions whose plan defines a
/// `quota_per_period` are counted; unlimited actions are never recorded here.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "subscription_usage")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i64,
  #[sea_orm(indexed)]
  pub subscription_id: i32,
  pub action: String,
  pub period_start: DateTimeWithTimeZone,
  #[sea_orm(default_value = 0)]
  pub used: i64,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
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
