use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::organizations::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct OrganizationFilter {
  pub name:      Option<String>,
  /// Which level of the tree is read. Saying nothing means the roots, not "every organization", so
  /// it carries a meaning a declared key cannot have — an unmentioned key is unfiltered. It stays a
  /// parameter of the list and never a field.
  pub parent_id: Option<i32>,
}

/// What `/organizations` accepts, declared once.
const ORGANIZATION_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::sort_only("id", FieldKind::Integer),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("updatedAt", FieldKind::DateTime),
];

impl FilterSchema for OrganizationFilter {
  fn fields() -> &'static [FieldSpec] {
    ORGANIZATION_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "name" => Column::Name.into_simple_expr(),
      "id" => Column::Id.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      "updatedAt" => Column::UpdatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "id"
  }
}

impl OrganizationFilter {
  pub fn apply<E>(&self, mut query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    query = query.apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)));

    query = if let Some(parent_id) = self.parent_id {
      query.filter(Column::ParentId.eq(parent_id))
    } else {
      query.filter(Column::ParentId.is_null())
    };

    query
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationListItem {
  pub id:         i32,
  pub name:       String,
  #[serde(default)]
  pub parent_id:  Nullable<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetOrganization {
  pub id:         i32,
  pub name:       String,
  pub parent_id:  Option<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetOrganization {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrganization {
  pub id:        Optional<i32>,
  pub name:      String,
  #[serde(default)]
  pub parent_id: Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationCreated {
  pub id: i32,
}

impl IntoResponse for OrganizationCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrganization {
  pub name:      Optional<String>,
  #[serde(default)]
  pub parent_id: Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationUpdated;

impl IntoResponse for OrganizationUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationDeleted;

impl IntoResponse for OrganizationDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    OrganizationFilter::validate().expect("the organization filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: OrganizationFilter = serde_urlencoded::from_str("name=a").expect("the flat filter parses");
    assert!(filter.name.is_some());
  }

  /// `parent_id` picks which level of the tree is read — its absence means the roots, which is not
  /// what an unmentioned key means — so it is not a field. Nothing may reach it.
  #[test]
  fn the_non_field_parameters_resolve_to_nothing() {
    for spelling in ["parent_id", "parentId", "parent", "created_at"] {
      assert!(OrganizationFilter::field(spelling).is_none(), "{spelling}");
      assert!(OrganizationFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(OrganizationFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["name", "id", "createdAt", "updatedAt"] {
      assert!(OrganizationFilter::sort_column(key).is_some(), "{key}");
    }
  }
}
