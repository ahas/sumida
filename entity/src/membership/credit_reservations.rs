use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::ReservationStatus;

/// A soft hold on credits (authorize-capture). `reserve` creates a `held` row that
/// reduces spendable balance without touching lots; `capture` debits the lots and
/// marks it `captured`; `release` (or expiry) frees the hold (`released`). Lots +
/// ledger remain authoritative; the wallet's spendable amount = `balance` minus the
/// sum of `held` reservations.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "credit_reservations")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:              i32,
  #[sea_orm(indexed)]
  pub wallet_id:       i32,
  pub amount:          i64,
  pub status:          ReservationStatus,
  #[sea_orm(nullable)]
  pub action:          Option<String>,
  #[sea_orm(nullable)]
  pub reference_code:  Option<String>,
  #[sea_orm(nullable)]
  pub idempotency_key: Option<String>,
  /// Optional auto-release deadline; the maintenance job releases held rows past it.
  #[sea_orm(nullable)]
  pub expires_at:      Option<DateTimeUtc>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:      DateTimeUtc,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at:      DateTimeUtc,
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
