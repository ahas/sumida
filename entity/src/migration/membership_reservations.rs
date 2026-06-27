use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// credit_reservations
#[derive(DeriveIden)]
enum CreditReservations {
  Table,
  Id,
  WalletId,
  Amount,
  Status,
  Action,
  ReferenceCode,
  IdempotencyKey,
  ExpiresAt,
  CreatedAt,
  UpdatedAt,
}

/// credit_wallets (existing) — FK target.
#[derive(DeriveIden)]
enum CreditWallets {
  Table,
  Id,
}

/// Additive follow-up to `membership`: credit reservations (authorize-capture).
/// A `held` row reduces spendable balance without touching lots until `capture`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CreditReservations::Table)
          .col(integer(CreditReservations::Id).auto_increment())
          .col(integer(CreditReservations::WalletId))
          .col(big_integer(CreditReservations::Amount))
          .col(string(CreditReservations::Status).default("held"))
          .col(string_null(CreditReservations::Action))
          .col(string_null(CreditReservations::ReferenceCode))
          .col(string_null(CreditReservations::IdempotencyKey))
          .col(timestamp_with_time_zone_null(CreditReservations::ExpiresAt))
          .col(timestamp_with_time_zone(CreditReservations::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(CreditReservations::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_credit_reservations__id").col(CreditReservations::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_credit_reservations__wallet_id")
              .from(CreditReservations::Table, CreditReservations::WalletId)
              .to(CreditWallets::Table, CreditWallets::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_credit_reservations__wallet__status")
          .table(CreditReservations::Table)
          .col(CreditReservations::WalletId)
          .col(CreditReservations::Status)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_credit_reservations__wallet__idempotency_key")
          .table(CreditReservations::Table)
          .col(CreditReservations::WalletId)
          .col(CreditReservations::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;

    attach_timestamp_auto_update(db, "credit_reservations").await?;
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    drop_table(manager, CreditReservations::Table).await
  }
}
