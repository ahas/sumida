use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::membership_plans::Column;
use entity::membership::{BillingInterval, OwnerKind, PlanKind};
use sea_orm::prelude::*;
use sea_orm::{Condition, FromQueryResult, QueryTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

fn default_true() -> bool {
  true
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFilter {
  pub search: Option<String>,
  pub kind: Option<PlanKind>,
  pub scope: Option<OwnerKind>,
  pub is_enabled: Option<bool>,
  /// Include soft-deleted (retired) plans. Defaults to false.
  #[serde(default)]
  pub include_deleted: bool,
}

impl PlanFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Column::Code.into_expr(), v))
            .add(wrap_ilike(Column::Name.into_expr(), v))
            .add(wrap_ilike(Column::Description.into_expr(), v)),
        )
      })
      .apply_if(self.kind.clone(), |q, v| q.filter(Column::Kind.eq(v)))
      .apply_if(self.scope.clone(), |q, v| q.filter(Column::Scope.eq(v)))
      .apply_if(self.is_enabled, |q, v| q.filter(Column::IsEnabled.eq(v)))
      .apply_if((!self.include_deleted).then_some(()), |q, _| q.filter(Column::DeletedAt.is_null()))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct PlanListItem {
  pub id: i32,
  pub code: String,
  pub name: String,
  pub description: Option<String>,
  pub kind: PlanKind,
  pub scope: OwnerKind,
  pub price: i64,
  pub currency: String,
  pub billing_interval: Option<BillingInterval>,
  pub duration_days: Option<i32>,
  pub credit_amount: Option<i64>,
  pub is_enabled: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct PlanActionItem {
  pub action: String,
  pub credit_cost: Option<i64>,
  pub quota_per_period: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPlan {
  #[serde(flatten)]
  pub plan: PlanListItem,
  pub actions: Vec<PlanActionItem>,
}

impl IntoResponse for GetPlan {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanActionInput {
  pub action: String,
  #[serde(default)]
  pub credit_cost: Option<i64>,
  #[serde(default)]
  pub quota_per_period: Option<i32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlan {
  pub code: String,
  pub name: String,
  #[serde(default)]
  pub description: Option<String>,
  pub kind: PlanKind,
  pub scope: OwnerKind,
  #[serde(default)]
  pub price: i64,
  #[serde(default)]
  pub currency: Option<String>,
  #[serde(default)]
  pub billing_interval: Option<BillingInterval>,
  #[serde(default)]
  pub duration_days: Option<i32>,
  #[serde(default)]
  pub credit_amount: Option<i64>,
  #[serde(default = "default_true")]
  pub is_enabled: bool,
  #[serde(default)]
  pub actions: Vec<PlanActionInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCreated {
  pub id: i32,
  pub code: String,
}

impl IntoResponse for PlanCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan {
  #[serde(default)]
  pub name: Option<String>,
  #[serde(default)]
  pub description: Option<String>,
  #[serde(default)]
  pub price: Option<i64>,
  #[serde(default)]
  pub currency: Option<String>,
  #[serde(default)]
  pub billing_interval: Option<BillingInterval>,
  #[serde(default)]
  pub duration_days: Option<i32>,
  #[serde(default)]
  pub credit_amount: Option<i64>,
  #[serde(default)]
  pub is_enabled: Option<bool>,
  /// When present, replaces the full action set.
  #[serde(default)]
  pub actions: Option<Vec<PlanActionInput>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanUpdated;

impl IntoResponse for PlanUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanDeleted;

impl IntoResponse for PlanDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
