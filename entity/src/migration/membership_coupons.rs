use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// coupons
#[derive(DeriveIden)]
enum Coupons {
  Table,
  Id,
  Code,
  PercentOff,
  AmountOff,
  Currency,
  MaxRedemptions,
  RedeemedCount,
  ExpiresAt,
  IsActive,
  CreatedAt,
  UpdatedAt,
}

/// membership_purchases (existing) — add the price breakdown columns.
#[derive(DeriveIden)]
enum MembershipPurchases {
  Table,
  Subtotal,
  DiscountAmount,
  TaxAmount,
  CouponCode,
}

/// Additive follow-up to `membership`: discount coupons + a price breakdown
/// (subtotal/discount/tax/coupon) on purchases for the checkout money path.
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
          .table(Coupons::Table)
          .col(integer(Coupons::Id).auto_increment())
          .col(string(Coupons::Code))
          .col(integer_null(Coupons::PercentOff))
          .col(big_integer_null(Coupons::AmountOff))
          .col(string_null(Coupons::Currency))
          .col(integer_null(Coupons::MaxRedemptions))
          .col(integer(Coupons::RedeemedCount).default(0))
          .col(timestamp_with_time_zone_null(Coupons::ExpiresAt))
          .col(boolean(Coupons::IsActive).default(true))
          .col(timestamp_with_time_zone(Coupons::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Coupons::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_coupons__id").col(Coupons::Id))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_coupons__code")
          .table(Coupons::Table)
          .col(Coupons::Code)
          .unique()
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "coupons").await?;

    for col in [
      MembershipPurchases::Subtotal,
      MembershipPurchases::DiscountAmount,
      MembershipPurchases::TaxAmount,
    ] {
      manager
        .alter_table(
          Table::alter()
            .table(MembershipPurchases::Table)
            .add_column_if_not_exists(big_integer(col).default(0))
            .to_owned(),
        )
        .await?;
    }
    manager
      .alter_table(
        Table::alter()
          .table(MembershipPurchases::Table)
          .add_column_if_not_exists(string_null(MembershipPurchases::CouponCode))
          .to_owned(),
      )
      .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    for col in [
      MembershipPurchases::Subtotal,
      MembershipPurchases::DiscountAmount,
      MembershipPurchases::TaxAmount,
      MembershipPurchases::CouponCode,
    ] {
      manager
        .alter_table(Table::alter().table(MembershipPurchases::Table).drop_column(col).to_owned())
        .await?;
    }
    drop_table(manager, Coupons::Table).await
  }
}
