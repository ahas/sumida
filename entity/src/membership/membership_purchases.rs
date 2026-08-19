use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use super::{OwnerKind, PlanKind, PurchaseStatus};

/// Canonical record of a purchase event ("무엇을 결제/구독했는지"). Subscriptions
/// and credit grants reference this via `reference_code` (= this row's `code`).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "membership_purchases")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:                  i32,
  #[sea_orm(indexed, unique)]
  pub code:                String,
  pub kind:                PlanKind,
  #[sea_orm(indexed)]
  pub plan_id:             i32,
  pub plan_code:           String,
  pub owner_kind:          OwnerKind,
  pub owner_id:            i32,
  pub status:              PurchaseStatus,
  /// Total charged, minor currency units (= subtotal - discount + tax).
  pub amount:              i64,
  /// List price before discount/tax (minor units). Defaults to `amount`.
  #[sea_orm(default_value = 0)]
  pub subtotal:            i64,
  /// Coupon discount applied (minor units).
  #[sea_orm(default_value = 0)]
  pub discount_amount:     i64,
  /// Tax added on the post-discount amount (minor units).
  #[sea_orm(default_value = 0)]
  pub tax_amount:          i64,
  /// Coupon code applied, if any.
  #[sea_orm(nullable)]
  pub coupon_code:         Option<String>,
  pub currency:            String,
  /// PG provider name ("none" for immediate/no-PG purchases).
  #[sea_orm(default_value = "'none'")]
  pub provider:            String,
  /// PG-side payment/transaction id once verified.
  #[sea_orm(nullable)]
  pub provider_payment_id: Option<String>,
  #[sea_orm(nullable)]
  pub idempotency_key:     Option<String>,
  #[sea_orm(nullable)]
  pub paid_at:             Option<DateTimeWithTimeZone>,
  #[sea_orm(nullable)]
  pub refunded_at:         Option<DateTimeWithTimeZone>,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub metadata:            Option<Json>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:          DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at:          DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
