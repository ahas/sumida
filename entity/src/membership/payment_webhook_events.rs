use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Idempotency log for processed PG webhooks (Standard Webhooks `webhook-id`).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "payment_webhook_events")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub provider: String,
  #[sea_orm(indexed, unique)]
  pub webhook_id: String,
  pub event_type: String,
  pub payment_id: String,
  #[sea_orm(nullable)]
  pub processed_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
