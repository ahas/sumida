use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// subscriptions (existing) — add auto-renew fields.
#[derive(DeriveIden)]
enum Subscriptions {
  Table,
  AutoRenew,
  BillingKey,
}

/// Additive follow-up to `membership`: auto-renew engine support. `auto_renew`
/// opts a subscription into the maintenance renewal job, which charges the stored
/// `billing_key` (정기결제) at period end via `PaymentProvider::charge_recurring`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Subscriptions::Table)
          .add_column_if_not_exists(boolean(Subscriptions::AutoRenew).default(false))
          .to_owned(),
      )
      .await?;
    manager
      .alter_table(
        Table::alter()
          .table(Subscriptions::Table)
          .add_column_if_not_exists(string_null(Subscriptions::BillingKey))
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
          .drop_column(Subscriptions::AutoRenew)
          .to_owned(),
      )
      .await?;
    manager
      .alter_table(
        Table::alter()
          .table(Subscriptions::Table)
          .drop_column(Subscriptions::BillingKey)
          .to_owned(),
      )
      .await?;
    Ok(())
  }
}
