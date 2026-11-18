use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::ReservationStatus;

/// A hold on a subscription action's amount quota for one period. A `held` row
/// takes its `amount` out of the period's room until it is captured into usage,
/// released, or passes `expires_at`. `period_start` is the period the hold was
/// taken in, and a capture records its usage there.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "quota_reservations")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:              i32,
  #[sea_orm(indexed)]
  pub subscription_id: i32,
  pub action:          String,
  pub period_start:    DateTimeUtc,
  pub amount:          i64,
  pub status:          ReservationStatus,
  #[sea_orm(nullable)]
  pub reference_code:  Option<String>,
  #[sea_orm(nullable)]
  pub idempotency_key: Option<String>,
  pub expires_at:      DateTimeUtc,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:      DateTimeUtc,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at:      DateTimeUtc,
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
