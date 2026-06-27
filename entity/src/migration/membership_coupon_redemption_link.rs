use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// coupon_redemptions (existing) — link a redemption to its purchase and the grant
/// it consumed, so a failed/abandoned checkout can be reversed.
#[derive(DeriveIden)]
enum CouponRedemptions {
  Table,
  PurchaseCode,
  GrantId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(CouponRedemptions::Table)
          .add_column_if_not_exists(string_null(CouponRedemptions::PurchaseCode))
          .to_owned(),
      )
      .await?;
    manager
      .alter_table(
        Table::alter()
          .table(CouponRedemptions::Table)
          .add_column_if_not_exists(integer_null(CouponRedemptions::GrantId))
          .to_owned(),
      )
      .await?;
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    for col in [CouponRedemptions::PurchaseCode, CouponRedemptions::GrantId] {
      manager
        .alter_table(Table::alter().table(CouponRedemptions::Table).drop_column(col).to_owned())
        .await?;
    }
    Ok(())
  }
}
