use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::OwnerKind;
use entity::membership::coupons::Column;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoSimpleExpr};
use serde::{Deserialize, Serialize};

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponFilter {
  pub is_active: Option<bool>,
}

/// What `/coupons` accepts, declared once.
///
/// The route is administrators only, and that is settled from the session before the list runs — no
/// parameter of this list picks a scope, so `isActive` is an ordinary narrowing filter.
const COUPON_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("isActive", FieldKind::Boolean, MatchStrategy::Comparison, &[Op::Eq]).with_facet(),
  FieldSpec::sort_only("code", FieldKind::Text),
  FieldSpec::sort_only("id", FieldKind::Integer),
];

impl FilterSchema for CouponFilter {
  fn fields() -> &'static [FieldSpec] {
    COUPON_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "isActive" => Column::IsActive.into_simple_expr(),
      "code" => Column::Code.into_simple_expr(),
      "id" => Column::Id.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "id"
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCoupon {
  pub code: String,
  /// Discount coupon: percentage off at checkout (0–100). Exactly one reward of
  /// `percent_off` / `grant_credits` / `free_plan_code` must be set.
  #[serde(default)]
  pub percent_off: Option<i32>,
  /// Reward coupon: credit units granted on redeem.
  #[serde(default)]
  pub grant_credits: Option<i64>,
  /// Reward coupon: subscription plan code granted free for one period on redeem.
  #[serde(default)]
  pub free_plan_code: Option<String>,
  /// Discount scope: only valid for purchases in this currency (regional promo).
  /// None = any currency.
  #[serde(default)]
  pub currency: Option<String>,
  #[serde(default)]
  pub max_redemptions: Option<i32>,
  /// Per-owner cap (e.g. 1 = once per customer).
  #[serde(default)]
  pub max_redemptions_per_owner: Option<i32>,
  /// When true, only owners the coupon is granted to may redeem it.
  #[serde(default)]
  pub requires_grant: bool,
  /// When true, combinable with other stackable coupons in one checkout.
  #[serde(default)]
  pub stackable: bool,
  /// Restrict the coupon to these plan codes (omit/empty = all plans).
  #[serde(default)]
  pub plan_codes: Option<Vec<String>>,
  #[serde(default)]
  pub expires_at: Option<DateTimeWithTimeZone>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct CouponItem {
  pub id: i32,
  pub code: String,
  pub percent_off: Option<i32>,
  pub grant_credits: Option<i64>,
  pub free_plan_code: Option<String>,
  pub currency: Option<String>,
  pub max_redemptions: Option<i32>,
  pub max_redemptions_per_owner: Option<i32>,
  pub redeemed_count: i32,
  pub requires_grant: bool,
  pub stackable: bool,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub is_active: bool,
  pub created_at: DateTimeWithTimeZone,
}

/// Issue a coupon to a specific owner.
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantCoupon {
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  pub owner_id:   i32,
}

/// Issue a coupon to many owners at once.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkGrantCoupon {
  pub owners: Vec<GrantCoupon>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkGrantResult {
  /// Newly issued or re-issued.
  pub granted:   usize,
  /// Already held an issued grant (idempotent no-op).
  pub skipped:   usize,
  /// Already redeemed — not re-issued (revoke first).
  pub conflicts: usize,
}

impl IntoResponse for BulkGrantResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

/// Result of applying one coupon during checkout.
pub struct AppliedCoupon {
  pub code:          String,
  pub discount:      i64,
  pub stackable:     bool,
  /// The redemption row created — checkout links it to the purchase afterwards.
  pub redemption_id: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponGranted;

impl IntoResponse for CouponGranted {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(serde_json::json!({}))).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponRevoked;

impl IntoResponse for CouponRevoked {
  fn into_response(self) -> axum::response::Response {
    Json(serde_json::json!({})).into_response()
  }
}

/// A coupon grant with its lifecycle timestamps (admin analytics). Status is
/// `revoked` if `revokedAt` is set, else `redeemed` if `redeemedAt` is set, else
/// `issued`.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct CouponGrantItem {
  pub id:          i32,
  pub coupon_id:   i32,
  pub owner_kind:  OwnerKind,
  pub owner_id:    i32,
  pub redeemed_at: Option<DateTimeWithTimeZone>,
  pub revoked_at:  Option<DateTimeWithTimeZone>,
  pub created_at:  DateTimeWithTimeZone,
}

/// A coupon available to the calling owner ("my coupons").
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct MyCouponItem {
  pub id:             i32,
  pub code:           String,
  pub percent_off:    Option<i32>,
  pub grant_credits:  Option<i64>,
  pub free_plan_code: Option<String>,
  pub currency:       Option<String>,
  pub expires_at:     Option<DateTimeWithTimeZone>,
}

/// Redeem a reward coupon (credits / free plan) for the calling user.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemCoupon {
  pub code: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemResult {
  /// How the coupon was redeemed: "credits" or "free-plan".
  pub reward:          String,
  pub credits_granted: Option<i64>,
  pub subscription_id: Option<i32>,
}

impl IntoResponse for RedeemResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponCreated {
  pub id:   i32,
  pub code: String,
}

impl IntoResponse for CouponCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

/// Partial update; only present fields are changed. Use `is_active=false` to retire
/// a coupon without deleting it.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCoupon {
  #[serde(default)]
  pub is_active:                 Option<bool>,
  #[serde(default)]
  pub max_redemptions:           Option<i32>,
  #[serde(default)]
  pub max_redemptions_per_owner: Option<i32>,
  #[serde(default)]
  pub requires_grant:            Option<bool>,
  #[serde(default)]
  pub stackable:                 Option<bool>,
  #[serde(default)]
  pub expires_at:                Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponUpdated;

impl IntoResponse for CouponUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(serde_json::json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct CouponDeleted;

impl IntoResponse for CouponDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(serde_json::json!({})).into_response()
  }
}

#[cfg(test)]
mod filter_tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    CouponFilter::validate().expect("the coupon filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: CouponFilter = serde_urlencoded::from_str("isActive=true").expect("the flat filter parses");
    assert_eq!(filter.is_active, Some(true));
  }

  /// The route refuses non-administrators from the session alone, not on the strength of any list
  /// parameter, so nothing is withheld on gate grounds. Every key the resource never declared stays
  /// unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["is_active", "redeemed_count", "percent_off", "expires_at", "requires_grant"] {
      assert!(CouponFilter::field(spelling).is_none(), "{spelling}");
      assert!(CouponFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(CouponFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(CouponFilter::sort_column("code").is_some());
    assert!(CouponFilter::sort_column("id").is_some());
    // Declared, but not sortable.
    assert!(CouponFilter::sort_column("isActive").is_none());
  }
}
