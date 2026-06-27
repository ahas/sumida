use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// subscriptions (existing) — add the dunning retry counter.
#[derive(DeriveIden)]
enum Subscriptions {
  Table,
  DunningAttempts,
}

/// Additive follow-up to `membership`: dunning. Counts failed renewal-charge
/// retries while a subscription is `past_due` within its grace window, so the
/// scheduler can cap retries (`membership.max_dunning_attempts`).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Subscriptions::Table)
          .add_column_if_not_exists(integer(Subscriptions::DunningAttempts).default(0))
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
          .drop_column(Subscriptions::DunningAttempts)
          .to_owned(),
      )
      .await?;
    Ok(())
  }
}
