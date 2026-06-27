use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-plan action configuration:
///
/// - subscription plan: a row means the action is allowed (`credit_cost` null);
///   `quota_per_period` optionally caps uses per billing period (null = unlimited).
/// - credit plan: `credit_cost` = number of credit units consumed per action.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "plan_actions")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(indexed)]
  pub plan_id: i32,
  pub action: String,
  #[sea_orm(nullable)]
  pub credit_cost: Option<i64>,
  #[sea_orm(nullable)]
  pub quota_per_period: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::membership_plans::Entity",
    from = "Column::PlanId",
    to = "super::membership_plans::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Plan,
}

impl Related<super::membership_plans::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Plan.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
