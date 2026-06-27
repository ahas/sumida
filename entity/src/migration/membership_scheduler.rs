use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// subscriptions (existing) — add the "expiring soon notified" marker.
#[derive(DeriveIden)]
enum Subscriptions {
  Table,
  ExpiringNotifiedAt,
}

/// credit_wallets (existing) — add the "low balance notified" marker.
#[derive(DeriveIden)]
enum CreditWallets {
  Table,
  LowBalanceNotifiedAt,
}

/// Additive follow-up to `membership`: dedup markers so the background scheduler
/// sends each "expiring soon" / "low balance" notice at most once per period
/// (cleared again on renew / top-up).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Subscriptions::Table)
          .add_column_if_not_exists(timestamp_with_time_zone_null(Subscriptions::ExpiringNotifiedAt))
          .to_owned(),
      )
      .await?;
    manager
      .alter_table(
        Table::alter()
          .table(CreditWallets::Table)
          .add_column_if_not_exists(timestamp_with_time_zone_null(CreditWallets::LowBalanceNotifiedAt))
          .to_owned(),
      )
      .await?;
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Subscriptions::Table)
          .drop_column(Subscriptions::ExpiringNotifiedAt)
          .to_owned(),
      )
      .await?;
    manager
      .alter_table(
        Table::alter()
          .table(CreditWallets::Table)
          .drop_column(CreditWallets::LowBalanceNotifiedAt)
          .to_owned(),
      )
      .await?;
    Ok(())
  }
}
