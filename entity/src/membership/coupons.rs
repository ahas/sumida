use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A discount coupon applied at checkout. Exactly one of `percent_off` (0–100) or
/// `amount_off` (minor units, in `currency`) should be set. Redemptions are capped
/// by `max_redemptions` (null = unlimited) and tracked in `redeemed_count`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "coupons")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(indexed, unique)]
  pub code: String,
  #[sea_orm(nullable)]
  pub percent_off: Option<i32>,
  #[sea_orm(nullable)]
  pub amount_off: Option<i64>,
  /// Required for `amount_off`: the currency it is denominated in.
  #[sea_orm(nullable)]
  pub currency: Option<String>,
  #[sea_orm(nullable)]
  pub max_redemptions: Option<i32>,
  /// Per-owner redemption cap (e.g. `1` = once per customer). None = unlimited.
  #[sea_orm(nullable)]
  pub max_redemptions_per_owner: Option<i32>,
  #[sea_orm(default_value = 0)]
  pub redeemed_count: i32,
  #[sea_orm(nullable)]
  pub expires_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = true)]
  pub is_active: bool,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
