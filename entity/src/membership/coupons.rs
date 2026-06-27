use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A membership coupon — exactly one reward: `percent_off` (a checkout discount),
/// `grant_credits` (redeem → credits), or `free_plan_code` (redeem → a free
/// subscription period). Redemptions are capped by `max_redemptions` (null =
/// unlimited) and tracked in `redeemed_count`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "coupons")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(indexed, unique)]
  pub code: String,
  /// Discount coupon: percentage off at checkout (0–100, currency-free).
  #[sea_orm(nullable)]
  pub percent_off: Option<i32>,
  /// Reward coupon: credit units granted on redeem.
  #[sea_orm(nullable)]
  pub grant_credits: Option<i64>,
  /// Reward coupon: a subscription plan code granted free for one period on redeem.
  #[sea_orm(nullable)]
  pub free_plan_code: Option<String>,
  /// Discount scope: applies only to purchases in this currency. None = any.
  #[sea_orm(nullable)]
  pub currency: Option<String>,
  #[sea_orm(nullable)]
  pub max_redemptions: Option<i32>,
  /// Per-owner redemption cap (e.g. `1` = once per customer). None = unlimited.
  #[sea_orm(nullable)]
  pub max_redemptions_per_owner: Option<i32>,
  #[sea_orm(default_value = 0)]
  pub redeemed_count: i32,
  /// When true, only owners the coupon was granted to may redeem it.
  #[sea_orm(default_value = false)]
  pub requires_grant: bool,
  /// When true, may be combined with other stackable coupons in one checkout.
  #[sea_orm(default_value = false)]
  pub stackable: bool,
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
