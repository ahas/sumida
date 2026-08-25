use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::SubscriptionEventKind;

/// Append-only audit log of subscription state transitions.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "subscription_events")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:                   i32,
  #[sea_orm(indexed)]
  pub subscription_id:      i32,
  pub event_kind:           SubscriptionEventKind,
  #[sea_orm(nullable)]
  pub old_status:           Option<String>,
  #[sea_orm(nullable)]
  pub new_status:           Option<String>,
  #[sea_orm(nullable)]
  pub reason:               Option<String>,
  #[sea_orm(nullable)]
  pub triggered_by_user_id: Option<i32>,
  #[sea_orm(nullable)]
  pub reference_code:       Option<String>,
  #[sea_orm(nullable)]
  pub idempotency_key:      Option<String>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:           DateTimeUtc,
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
