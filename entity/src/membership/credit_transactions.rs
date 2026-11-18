use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::CreditTxKind;

/// Append-only credit ledger. `amount` is the signed change to the balance (grant > 0,
/// consume/expire < 0) and `debt_change` the signed change to the debt (positive for a
/// short capture, negative for a repayment), so the sums of the two are the wallet's
/// balance and debt.
/// `unique(wallet_id, idempotency_key)` enforces per-wallet idempotency.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credit_transactions")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:              i64,
  #[sea_orm(indexed)]
  pub wallet_id:       i32,
  #[sea_orm(nullable)]
  pub grant_id:        Option<i32>,
  #[sea_orm(nullable)]
  pub subscription_id: Option<i32>,
  pub kind:            CreditTxKind,
  pub amount:          i64,
  #[sea_orm(default_value = 0)]
  pub debt_change:     i64,
  #[sea_orm(nullable)]
  pub action:          Option<String>,
  #[sea_orm(nullable)]
  pub reference_code:  Option<String>,
  #[sea_orm(nullable)]
  pub idempotency_key: Option<String>,
  pub balance_after:   i64,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:      DateTimeUtc,
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
