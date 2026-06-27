use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// coupons (existing) — add a per-owner redemption cap.
#[derive(DeriveIden)]
enum Coupons {
  Table,
  MaxRedemptionsPerOwner,
}

/// Additive follow-up to `membership_coupons`: cap redemptions per owner (e.g.
/// `1` = once per customer), counted against prior non-failed purchases bearing
/// the coupon code.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Coupons::Table)
          .add_column_if_not_exists(integer_null(Coupons::MaxRedemptionsPerOwner))
          .to_owned(),
      )
      .await?;
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Coupons::Table)
          .drop_column(Coupons::MaxRedemptionsPerOwner)
          .to_owned(),
      )
      .await?;
    Ok(())
  }
}
