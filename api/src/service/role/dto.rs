use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::roles::{ActiveModel, Column};
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, IntoSimpleExpr, QueryTrait, Set};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::grants::ApForUser;
use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RoleFilter {
  pub name:        Option<String>,
  pub description: Option<String>,
}

impl RoleFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(self.description.as_ref(), |q, v| {
        q.filter(wrap_ilike(Column::Description.into_expr(), v))
      })
  }
}

/// What `/roles` accepts, declared once.
const ROLE_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("description", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::sort_only("id", FieldKind::Integer),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("updatedAt", FieldKind::DateTime),
];

impl FilterSchema for RoleFilter {
  fn fields() -> &'static [FieldSpec] {
    ROLE_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "name" => Column::Name.into_simple_expr(),
      "description" => Column::Description.into_simple_expr(),
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

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct RoleListItem {
  pub id:          i32,
  pub name:        String,
  pub description: Option<String>,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetRole {
  pub id:              i32,
  pub name:            String,
  pub description:     Option<String>,
  #[sea_orm(skip)]
  pub access_policies: Vec<ApForUser>,
  pub created_at:      DateTimeWithTimeZone,
  pub updated_at:      DateTimeWithTimeZone,
}

impl IntoResponse for GetRole {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRole {
  pub id:              Optional<i32>,
  pub name:            String,
  #[serde(default)]
  pub description:     Nullable<String>,
  pub access_policies: Vec<ApForUser>,
}

impl IntoActiveModel<ActiveModel> for CreateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      name: Set(self.name),
      description: self.description.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCreated {
  pub id: i32,
}

impl IntoResponse for RoleCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRole {
  pub name:            Optional<String>,
  #[serde(default)]
  pub description:     Nullable<String>,
  pub access_policies: Option<Vec<ApForUser>>,
}

impl IntoActiveModel<ActiveModel> for UpdateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      name: self.name.into_active_value(),
      description: self.description.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleUpdated;

impl IntoResponse for RoleUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDeleted;

impl IntoResponse for RoleDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    RoleFilter::validate().expect("the role filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: RoleFilter =
      serde_urlencoded::from_str("name=a&description=b").expect("the flat filter parses");

    assert!(filter.name.is_some());
    assert!(filter.description.is_some());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["created_at", "updated_at", "access_policies", "user_id"] {
      assert!(RoleFilter::field(spelling).is_none(), "{spelling}");
      assert!(RoleFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(RoleFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["name", "id", "createdAt", "updatedAt"] {
      assert!(RoleFilter::sort_column(key).is_some(), "{key}");
    }
    // Declared, but not sortable.
    assert!(RoleFilter::sort_column("description").is_none());
  }
}
