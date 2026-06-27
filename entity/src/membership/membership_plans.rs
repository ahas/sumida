use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::{BillingInterval, OwnerKind, PlanKind};

/// Catalog of purchasable plans (subscription or credit). Soft-deleted via
/// `deleted_at`; never hard-deleted while referenced by subscriptions/purchases.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "membership_plans")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(indexed, unique)]
  pub code: String,
  pub name: String,
  #[sea_orm(nullable)]
  pub description: Option<String>,
  pub kind: PlanKind,
  pub scope: OwnerKind,
  /// Minor currency units (e.g. KRW).
  #[sea_orm(default_value = 0)]
  pub price: i64,
  #[sea_orm(default_value = "'KRW'")]
  pub currency: String,
  #[sea_orm(nullable)]
  pub billing_interval: Option<BillingInterval>,
  /// Authoritative subscription length in days.
  #[sea_orm(nullable)]
  pub duration_days: Option<i32>,
  /// Free-trial length in days for a new subscription's first period. None = no trial.
  #[sea_orm(nullable)]
  pub trial_days: Option<i32>,
  /// Credit units granted when this (credit) plan is purchased.
  #[sea_orm(nullable)]
  pub credit_amount: Option<i64>,
  #[sea_orm(default_value = true)]
  pub is_enabled: bool,
  #[sea_orm(nullable)]
  pub provider_product_id: Option<String>,
  #[sea_orm(nullable)]
  pub deleted_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(has_many = "super::plan_actions::Entity")]
  Actions,
}

impl Related<super::plan_actions::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Actions.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
