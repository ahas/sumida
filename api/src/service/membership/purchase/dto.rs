use entity::membership::{OwnerKind, PlanKind, PurchaseStatus};
use sea_orm::prelude::*;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseFilter {
  pub owner_kind: Option<OwnerKind>,
  pub owner_id: Option<i32>,
  pub kind: Option<PlanKind>,
  pub status: Option<PurchaseStatus>,
  pub plan_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseListItem {
  pub id: i32,
  pub code: String,
  pub kind: PlanKind,
  pub plan_id: i32,
  pub plan_code: String,
  pub owner_kind: OwnerKind,
  pub owner_id: i32,
  pub status: PurchaseStatus,
  pub amount: i64,
  pub currency: String,
  pub paid_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
}
