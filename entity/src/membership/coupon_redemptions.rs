use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::OwnerKind;

/// One row per coupon redemption by an owner — the source for per-owner caps and
/// issuance/usage analytics (independent of how many coupons a checkout stacks).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "coupon_redemptions")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:            i32,
  #[sea_orm(indexed)]
  pub coupon_id:     i32,
  pub owner_kind:    OwnerKind,
  pub owner_id:      i32,
  /// The purchase this redemption belongs to (set once the purchase exists).
  #[sea_orm(nullable)]
  pub purchase_code: Option<String>,
  /// The grant consumed by this redemption, if the coupon is grant-gated.
  #[sea_orm(nullable)]
  pub grant_id:      Option<i32>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:    DateTimeUtc,
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
