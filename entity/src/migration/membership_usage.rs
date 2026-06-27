use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// subscription_usage
#[derive(DeriveIden)]
enum SubscriptionUsage {
  Table,
  Id,
  SubscriptionId,
  Action,
  PeriodStart,
  Used,
  CreatedAt,
  UpdatedAt,
}

/// subscriptions (existing) — FK target.
#[derive(DeriveIden)]
enum Subscriptions {
  Table,
  Id,
}

/// Additive follow-up to `membership`: per-period usage counters enforcing
/// `plan_actions.quota_per_period` on subscription actions. Unique per
/// (subscription, action, period_start) so quota resets each period.
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
          .table(SubscriptionUsage::Table)
          .col(big_integer(SubscriptionUsage::Id).auto_increment())
          .col(integer(SubscriptionUsage::SubscriptionId))
          .col(string(SubscriptionUsage::Action))
          .col(timestamp_with_time_zone(SubscriptionUsage::PeriodStart))
          .col(big_integer(SubscriptionUsage::Used).default(0))
          .col(timestamp_with_time_zone(SubscriptionUsage::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(SubscriptionUsage::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_subscription_usage__id").col(SubscriptionUsage::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_subscription_usage__subscription_id")
              .from(SubscriptionUsage::Table, SubscriptionUsage::SubscriptionId)
              .to(Subscriptions::Table, Subscriptions::Id)
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
          .name("uq_subscription_usage__sub__action__period")
          .table(SubscriptionUsage::Table)
          .col(SubscriptionUsage::SubscriptionId)
          .col(SubscriptionUsage::Action)
          .col(SubscriptionUsage::PeriodStart)
          .unique()
          .to_owned(),
      )
      .await?;

    attach_timestamp_auto_update(db, "subscription_usage").await?;
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    drop_table(manager, SubscriptionUsage::Table).await
  }
}
