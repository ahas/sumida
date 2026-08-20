use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_groups::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{DeriveIntoActiveModel, EntityTrait, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardGroupFilter {
  /// A name to look for anywhere in the tree. It is what a reader types when they cannot find
  /// something, so it answers from every level rather than from the one being read.
  pub search:    Option<String>,
  pub name:      Option<OneOrMany<String>>,
  /// Which level of the tree is read. Saying nothing means the roots, not "every group", so it
  /// carries a meaning a declared key cannot have — an unmentioned key is unfiltered. It stays a
  /// parameter of the list and never a field.
  pub parent_id: Option<i32>,
}

impl BoardGroupFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    // Reading a level and searching for a name are different questions. A level was asked for, so
    // that level answers; a search with no level named answers from the whole tree, because the
    // node a reader is searching for is precisely one they have not opened to. With neither, the
    // roots answer, which is where reading a tree starts.
    let query = match (self.parent_id, self.search.is_some()) {
      (Some(parent_id), _) => query.filter(Column::ParentId.eq(parent_id)),
      (None, true) => query,
      (None, false) => query.filter(Column::ParentId.is_null()),
    };

    query
      .apply_if(self.search.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(listed(self.name.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::Name)))
  }
}

/// What `/board-groups` accepts, declared once.
///
/// `search` and `name` both stand on the group name and differ only in how they match it — one
/// wraps an `ILIKE`, the other compares.
const BOARD_GROUP_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_sort(),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for BoardGroupFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_GROUP_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "search" | "name" => Column::Name.into_simple_expr(),
      "created_at" => Column::CreatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "created_at"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupListItem {
  pub id:          i32,
  pub name:        String,
  pub description: Option<String>,
  pub order:       i32,
  pub parent_id:   Option<i32>,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardGroup {
  pub id:          i32,
  pub parent_id:   Option<i32>,
  pub name:        String,
  pub description: Option<String>,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardGroup {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardGroup {
  pub id:          Optional<i32>,
  pub name:        String,
  pub description: Optional<String>,
  #[serde(default)]
  pub parent_id:   Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupCreated {
  pub id: i32,
}

impl IntoResponse for BoardGroupCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardGroup {
  pub name:        Optional<String>,
  pub description: Optional<String>,
  #[serde(default)]
  pub parent_id:   Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupUpdated;

impl IntoResponse for BoardGroupUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupDeleted;

impl IntoResponse for BoardGroupDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    BoardGroupFilter::validate().expect("the board group filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: BoardGroupFilter =
      serde_urlencoded::from_str("search=a&name=b").expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.name.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardGroupFilter = serde_urlencoded::from_str("name=a,b").expect("a list parses");
    assert_eq!(filter.name.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardGroupFilter = serde_urlencoded::from_str("name=").expect("an empty value parses");
    assert!(listed(filter.name.as_ref()).is_none());
  }

  /// `parent_id` picks which level of the tree is read — its absence means the roots, which is not
  /// what an unmentioned key means — so it is not a field. Nothing may reach it, and the reserved
  /// `order` key that drives the display sequence is not reachable either.
  #[test]
  fn the_non_field_parameters_resolve_to_nothing() {
    for spelling in ["parent_id", "parentId", "parent", "order", "id"] {
      assert!(BoardGroupFilter::field(spelling).is_none(), "{spelling}");
      assert!(BoardGroupFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BoardGroupFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(BoardGroupFilter::sort_column("name").is_some());
    assert!(BoardGroupFilter::sort_column("created_at").is_some());
    assert!(BoardGroupFilter::sort_column("search").is_none());
  }
}
