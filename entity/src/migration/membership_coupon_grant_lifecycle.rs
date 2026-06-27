use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// coupon_grants (existing) — add redemption / revocation timestamps.
#[derive(DeriveIden)]
enum CouponGrants {
  Table,
  RedeemedAt,
  RevokedAt,
}

/// Additive follow-up to `membership_coupon_grants`: a grant lifecycle. A grant is
/// `issued` (both null), `redeemed` (redeemed_at set when consumed at checkout), or
/// `revoked` (revoked_at set on recall). For grant-gated coupons one grant = one
/// use; re-issuing a used grant requires revoking it first.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    for col in [CouponGrants::RedeemedAt, CouponGrants::RevokedAt] {
      manager
        .alter_table(
          Table::alter()
            .table(CouponGrants::Table)
            .add_column_if_not_exists(timestamp_with_time_zone_null(col))
            .to_owned(),
        )
        .await?;
    }
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    for col in [CouponGrants::RedeemedAt, CouponGrants::RevokedAt] {
      manager
        .alter_table(Table::alter().table(CouponGrants::Table).drop_column(col).to_owned())
        .await?;
    }
    Ok(())
  }
}
