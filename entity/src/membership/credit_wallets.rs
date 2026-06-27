use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::OwnerKind;

/// One wallet per owner (plan-agnostic shared pool). `balance` is a cache of the
/// sum of non-expired grant lots' `amount_remaining`; the lots + ledger are
/// authoritative.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credit_wallets")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub currency: String,
  #[sea_orm(default_value = 0)]
  pub balance: i64,
  /// When a "low balance" alert was last sent. Cleared on top-up above the
  /// configured threshold so a later drop can alert again.
  #[sea_orm(nullable)]
  pub low_balance_notified_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(has_many = "super::credit_grants::Entity")]
  Grants,
  #[sea_orm(has_many = "super::credit_transactions::Entity")]
  Transactions,
}

impl Related<super::credit_grants::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Grants.def()
  }
}

impl Related<super::credit_transactions::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Transactions.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
