use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::membership_plans::Column;
use entity::membership::{BillingInterval, OwnerKind, PlanKind};
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{ActiveEnum, Condition, FromQueryResult, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

fn default_true() -> bool {
  true
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFilter {
  pub search:          Option<String>,
  pub kind:            Option<OneOrMany<PlanKind>>,
  pub scope:           Option<OneOrMany<OwnerKind>>,
  pub is_enabled:      Option<bool>,
  /// Include soft-deleted (retired) plans. Defaults to false.
  ///
  /// It toggles whether the soft-delete predicate is emitted at all, so it reduces to no expression
  /// under any operator, and saying nothing means "exclude retired plans" rather than unfiltered —
  /// the opposite of what an unmentioned key means. It stays a parameter of the list, never a field.
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
      .apply_if(listed(self.kind.as_ref()), |q, v| {
        q.filter(Column::Kind.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(self.scope.as_ref()), |q, v| {
        q.filter(Column::Scope.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(self.is_enabled, |q, v| q.filter(Column::IsEnabled.eq(v)))
      .apply_if((!self.include_deleted).then_some(()), |q, _| q.filter(Column::DeletedAt.is_null()))
  }
}

/// What `/membership-plans` accepts, declared once.
///
/// `search` widens the code, the name and the description into one predicate, so it reduces to no
/// single expression and the resource builds it.
const PLAN_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("kind", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["subscription", "credit"]),
  FieldSpec::new("scope", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["personal", "organization"]),
  FieldSpec::new("isEnabled", FieldKind::Boolean, MatchStrategy::Comparison, &[Op::Eq]),
  FieldSpec::sort_only("code", FieldKind::Text),
  FieldSpec::sort_only("name", FieldKind::Text),
  FieldSpec::sort_only("price", FieldKind::Integer),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("id", FieldKind::Integer),
];

impl FilterSchema for PlanFilter {
  fn fields() -> &'static [FieldSpec] {
    PLAN_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "kind" => Column::Kind.into_simple_expr(),
      "scope" => Column::Scope.into_simple_expr(),
      "isEnabled" => Column::IsEnabled.into_simple_expr(),
      "code" => Column::Code.into_simple_expr(),
      "name" => Column::Name.into_simple_expr(),
      "price" => Column::Price.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      "id" => Column::Id.into_simple_expr(),
      // `search` spans three columns, so it has no single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "id"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct PlanListItem {
  pub id:               i32,
  pub code:             String,
  pub name:             String,
  pub description:      Option<String>,
  pub kind:             PlanKind,
  pub scope:            OwnerKind,
  pub price:            i64,
  pub currency:         String,
  pub billing_interval: Option<BillingInterval>,
  pub duration_days:    Option<i32>,
  pub trial_days:       Option<i32>,
  pub credit_amount:    Option<i64>,
  pub is_enabled:       bool,
  pub created_at:       DateTimeWithTimeZone,
  pub updated_at:       DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct PlanActionItem {
  pub action:           String,
  pub credit_cost:      Option<i64>,
  pub quota_per_period: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPlan {
  #[serde(flatten)]
  pub plan:    PlanListItem,
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
  pub action:           String,
  #[serde(default)]
  pub credit_cost:      Option<i64>,
  #[serde(default)]
  pub quota_per_period: Option<i32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlan {
  pub code:             String,
  pub name:             String,
  #[serde(default)]
  pub description:      Option<String>,
  pub kind:             PlanKind,
  pub scope:            OwnerKind,
  #[serde(default)]
  pub price:            i64,
  #[serde(default)]
  pub currency:         Option<String>,
  #[serde(default)]
  pub billing_interval: Option<BillingInterval>,
  #[serde(default)]
  pub duration_days:    Option<i32>,
  #[serde(default)]
  pub trial_days:       Option<i32>,
  #[serde(default)]
  pub credit_amount:    Option<i64>,
  #[serde(default = "default_true")]
  pub is_enabled:       bool,
  #[serde(default)]
  pub actions:          Vec<PlanActionInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCreated {
  pub id:   i32,
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
  pub name:             Option<String>,
  #[serde(default)]
  pub description:      Option<String>,
  #[serde(default)]
  pub price:            Option<i64>,
  #[serde(default)]
  pub currency:         Option<String>,
  #[serde(default)]
  pub billing_interval: Option<BillingInterval>,
  #[serde(default)]
  pub duration_days:    Option<i32>,
  #[serde(default)]
  pub trial_days:       Option<i32>,
  #[serde(default)]
  pub credit_amount:    Option<i64>,
  #[serde(default)]
  pub is_enabled:       Option<bool>,
  /// When present, replaces the full action set.
  #[serde(default)]
  pub actions:          Option<Vec<PlanActionInput>>,
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

#[cfg(test)]
mod filter_tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    PlanFilter::validate().expect("the plan filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "search=a&kind=credit&scope=personal&isEnabled=true";
    let filter: PlanFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.kind.is_some());
    assert!(filter.scope.is_some());
    assert_eq!(filter.is_enabled, Some(true));
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: PlanFilter =
      serde_urlencoded::from_str("kind=subscription,credit&scope=personal,organization")
        .expect("a list parses");

    assert_eq!(filter.kind.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.scope.as_ref().map(OneOrMany::len), Some(2));

    let filter: PlanFilter = serde_urlencoded::from_str("kind=&scope=").expect("an empty value parses");

    assert!(listed(filter.kind.as_ref()).is_none());
    assert!(listed(filter.scope.as_ref()).is_none());
  }

  #[test]
  fn an_unknown_enum_variant_is_refused() {
    assert!(serde_urlencoded::from_str::<PlanFilter>("kind=barter").is_err());
    assert!(serde_urlencoded::from_str::<PlanFilter>("scope=personal,galactic").is_err());
  }

  /// Every variant the flat parameters parse is a variant the declaration names, so the two paths
  /// cannot disagree about what these keys accept.
  #[test]
  fn the_declared_variants_are_the_ones_the_parameters_parse() {
    for key in ["kind", "scope"] {
      let field = PlanFilter::field(key).unwrap_or_else(|| panic!("`{key}` is declared"));

      for variant in field.variants {
        serde_urlencoded::from_str::<PlanFilter>(&format!("{key}={variant}"))
          .unwrap_or_else(|_| panic!("`{key}={variant}` must parse"));
      }
    }
  }

  /// `includeDeleted` toggles whether the soft-delete predicate is emitted at all, and its absence
  /// narrows rather than widens, so it is not a field. Nothing may reach it.
  #[test]
  fn the_non_field_parameters_resolve_to_nothing() {
    for spelling in ["includeDeleted", "include_deleted", "deleted_at", "is_enabled"] {
      assert!(PlanFilter::field(spelling).is_none(), "{spelling}");
      assert!(PlanFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(PlanFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["code", "name", "price", "createdAt", "id"] {
      assert!(PlanFilter::sort_column(key).is_some(), "{key}");
    }
    for key in ["search", "kind", "scope", "isEnabled"] {
      assert!(PlanFilter::sort_column(key).is_none(), "{key}");
    }
  }
}
