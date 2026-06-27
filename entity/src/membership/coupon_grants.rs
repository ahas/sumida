use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::OwnerKind;

/// A coupon issued to a specific owner. When the coupon's `requires_grant` is set,
/// only owners with a grant may redeem it; grants also back the "my coupons" list.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "coupon_grants")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(indexed)]
  pub coupon_id: i32,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  /// Set when consumed at checkout (one grant = one use for grant-gated coupons).
  #[sea_orm(nullable)]
  pub redeemed_at: Option<DateTimeWithTimeZone>,
  /// Set on recall. A revoked grant cannot be redeemed; re-granting re-issues it.
  #[sea_orm(nullable)]
  pub revoked_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::coupons::Entity",
    from = "Column::CouponId",
    to = "super::coupons::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Coupon,
}

impl Related<super::coupons::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Coupon.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
