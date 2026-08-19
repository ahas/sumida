use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Restricts a coupon to a plan. A coupon with no `coupon_plans` rows applies to
/// any plan; otherwise it is valid only for the listed plan codes.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "coupon_plans")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:        i32,
  #[sea_orm(indexed)]
  pub coupon_id: i32,
  pub plan_code: String,
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
