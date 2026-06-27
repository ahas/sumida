use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

use super::helper::*;

/// coupon_grants — a coupon issued to a specific owner ("coupon wallet").
#[derive(DeriveIden)]
enum CouponGrants {
  Table,
  Id,
  CouponId,
  OwnerKind,
  OwnerId,
  CreatedAt,
}

/// coupons (existing) — gate redemption behind an explicit grant.
#[derive(DeriveIden)]
enum Coupons {
  Table,
  Id,
  RequiresGrant,
}

/// Additive follow-up to `membership_coupons`: coupon distribution. `requires_grant`
/// makes a coupon redeemable only by owners it was issued to; `coupon_grants` is the
/// per-owner issuance record (used both for that gate and for "my coupons").
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(Coupons::Table)
          .add_column_if_not_exists(boolean(Coupons::RequiresGrant).default(false))
          .to_owned(),
      )
      .await?;

    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CouponGrants::Table)
          .col(integer(CouponGrants::Id).auto_increment())
          .col(integer(CouponGrants::CouponId))
          .col(string(CouponGrants::OwnerKind))
          .col(integer(CouponGrants::OwnerId))
          .col(timestamp_with_time_zone(CouponGrants::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_coupon_grants__id").col(CouponGrants::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_coupon_grants__coupon_id")
              .from(CouponGrants::Table, CouponGrants::CouponId)
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
          .name("uq_coupon_grants__coupon__owner")
          .table(CouponGrants::Table)
          .col(CouponGrants::CouponId)
          .col(CouponGrants::OwnerKind)
          .col(CouponGrants::OwnerId)
          .unique()
          .to_owned(),
      )
      .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    drop_table(manager, CouponGrants::Table).await?;
    manager
      .alter_table(
        Table::alter().table(Coupons::Table).drop_column(Coupons::RequiresGrant).to_owned(),
      )
      .await?;
    Ok(())
  }
}
