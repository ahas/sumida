use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// coupons (existing) — add stacking.
#[derive(DeriveIden)]
enum Coupons {
  Table,
  Id,
  Stackable,
}

/// coupon_plans — restricts a coupon to specific plan codes (empty = all plans).
#[derive(DeriveIden)]
enum CouponPlans {
  Table,
  Id,
  CouponId,
  PlanCode,
}

/// coupon_redemptions — one row per coupon use by an owner (per-owner cap + analytics).
#[derive(DeriveIden)]
enum CouponRedemptions {
  Table,
  Id,
  CouponId,
  OwnerKind,
  OwnerId,
  CreatedAt,
}

/// Additive follow-up: coupon stacking, plan-scoping, and a redemption ledger
/// (robust per-owner counting that also survives multiple coupons per checkout).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Coupons::Table)
          .add_column_if_not_exists(boolean(Coupons::Stackable).default(false))
          .to_owned(),
      )
      .await?;

    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CouponPlans::Table)
          .col(integer(CouponPlans::Id).auto_increment())
          .col(integer(CouponPlans::CouponId))
          .col(string(CouponPlans::PlanCode))
          .primary_key(Index::create().name("pk_coupon_plans__id").col(CouponPlans::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_coupon_plans__coupon_id")
              .from(CouponPlans::Table, CouponPlans::CouponId)
              .to(Coupons::Table, Coupons::Id)
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
          .name("uq_coupon_plans__coupon__plan")
          .table(CouponPlans::Table)
          .col(CouponPlans::CouponId)
          .col(CouponPlans::PlanCode)
          .unique()
          .to_owned(),
      )
      .await?;

    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CouponRedemptions::Table)
          .col(integer(CouponRedemptions::Id).auto_increment())
          .col(integer(CouponRedemptions::CouponId))
          .col(string(CouponRedemptions::OwnerKind))
          .col(integer(CouponRedemptions::OwnerId))
          .col(timestamp_with_time_zone(CouponRedemptions::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_coupon_redemptions__id").col(CouponRedemptions::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_coupon_redemptions__coupon_id")
              .from(CouponRedemptions::Table, CouponRedemptions::CouponId)
              .to(Coupons::Table, Coupons::Id)
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
          .name("idx_coupon_redemptions__coupon__owner")
          .table(CouponRedemptions::Table)
          .col(CouponRedemptions::CouponId)
          .col(CouponRedemptions::OwnerKind)
          .col(CouponRedemptions::OwnerId)
          .to_owned(),
      )
      .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .drop_table(Table::drop().if_exists().cascade().table(CouponRedemptions::Table).to_owned())
      .await?;
    manager
      .drop_table(Table::drop().if_exists().cascade().table(CouponPlans::Table).to_owned())
      .await?;
    manager
      .alter_table(Table::alter().table(Coupons::Table).drop_column(Coupons::Stackable).to_owned())
      .await?;
    Ok(())
  }
}
