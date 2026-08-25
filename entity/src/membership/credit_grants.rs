use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A granted credit lot. Consumption draws FIFO across non-expired lots ordered
/// by `(expiry_at NULLS LAST, created_at)`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credit_grants")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:               i32,
  #[sea_orm(indexed)]
  pub wallet_id:        i32,
  #[sea_orm(nullable)]
  pub source_plan_code: Option<String>,
  pub amount_granted:   i64,
  pub amount_remaining: i64,
  #[sea_orm(nullable)]
  pub expiry_at:        Option<DateTimeUtc>,
  #[sea_orm(nullable)]
  pub reference_code:   Option<String>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:       DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::credit_wallets::Entity",
    from = "Column::WalletId",
    to = "super::credit_wallets::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Wallet,
}

impl Related<super::credit_wallets::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Wallet.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
