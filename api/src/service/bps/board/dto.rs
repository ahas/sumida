use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::boards::*;
use entity::prelude::*;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{Condition, EntityTrait, IntoActiveModel, IntoActiveValue, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardFilter {
  pub search:   Option<String>,
  pub name:     Option<String>,
  pub group_id: Option<OneOrMany<i32>>,
}

impl BoardFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Column::Name.into_expr(), v))
            .add(wrap_ilike(Column::Description.into_expr(), v)),
        )
      })
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(listed(self.group_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::GroupId)))
  }
}

/// What `/boards` accepts, declared once.
///
/// `search` spans the name and the description, so it reduces to no single expression and the
/// resource builds its predicate itself; `name` is the same match against one column and reduces
/// to it, which is why the two carry different strategies.
const BOARD_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("group_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/board-groups", "name"),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for BoardFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "name" => Column::Name.into_simple_expr(),
      "group_id" => Column::GroupId.into_simple_expr(),
      "created_at" => Column::CreatedAt.into_simple_expr(),
      // `search` spans the name and the description, so it has no single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "created_at"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardGroups", from_query_result)]
pub struct BoardBoardGroup {
  pub id:   i32,
  pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Boards", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardListItem {
  pub id:              i32,
  #[sea_orm(nested)]
  pub group:           Option<BoardBoardGroup>,
  pub name:            String,
  pub description:     Option<String>,
  pub order:           i32,
  pub can_upload_file: bool,
  pub is_versioned:    bool,
  pub is_enabled:      bool,
  pub created_at:      DateTimeWithTimeZone,
  pub updated_at:      DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Boards", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoard {
  pub id:                  i32,
  #[sea_orm(nested)]
  pub group:               Option<BoardBoardGroup>,
  pub name:                String,
  pub description:         Option<String>,
  pub order:               i32,
  pub can_upload_file:     bool,
  pub max_file_size:       i64,
  pub max_file_count:      i32,
  pub allowed_file_types:  Option<JsonValue>,
  pub reaction_visibility: BoardPostRecordVisibility,
  pub viewer_visibility:   BoardPostRecordVisibility,
  pub content_visibility:  BoardPostRecordVisibility,
  pub is_versioned:        bool,
  pub is_enabled:          bool,
  pub created_at:          DateTimeWithTimeZone,
  pub updated_at:          DateTimeWithTimeZone,
}

impl IntoResponse for GetBoard {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoard {
  pub id:                  Optional<i32>,
  #[serde(default)]
  pub group_id:            Nullable<i32>,
  #[serde(default)]
  pub topics:              Nullable<Vec<String>>,
  pub name:                String,
  #[serde(default)]
  pub description:         Nullable<String>,
  pub order:               Optional<i32>,
  pub can_upload_file:     Optional<bool>,
  pub max_file_size:       Optional<i64>,
  pub max_file_count:      Optional<i32>,
  #[serde(default)]
  pub allowed_file_types:  Nullable<JsonValue>,
  pub reaction_visibility: Optional<BoardPostRecordVisibility>,
  pub viewer_visibility:   Optional<BoardPostRecordVisibility>,
  pub is_versioned:        Optional<bool>,
  pub is_enabled:          Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for CreateBoard {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      group_id: self.group_id.into_active_value(),
      name: Set(self.name),
      description: self.description.into_active_value(),
      order: self.order.into_active_value(),
      can_upload_file: self.can_upload_file.into_active_value(),
      max_file_size: self.max_file_size.into_active_value(),
      max_file_count: self.max_file_count.into_active_value(),
      allowed_file_types: self.allowed_file_types.into_active_value(),
      reaction_visibility: self.reaction_visibility.into_active_value(),
      viewer_visibility: self.viewer_visibility.into_active_value(),
      is_versioned: self.is_versioned.into_active_value(),
      is_enabled: self.is_enabled.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCreated {
  pub id: i32,
}

impl IntoResponse for BoardCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoard {
  #[serde(default)]
  pub group_id:            Nullable<i32>,
  #[serde(default)]
  pub topics:              Nullable<Vec<String>>,
  pub name:                Optional<String>,
  pub description:         Nullable<String>,
  pub order:               Optional<i32>,
  pub can_upload_file:     Optional<bool>,
  pub max_file_size:       Optional<i64>,
  pub max_file_count:      Optional<i32>,
  #[serde(default)]
  pub allowed_file_types:  Nullable<JsonValue>,
  pub reaction_visibility: Optional<BoardPostRecordVisibility>,
  pub viewer_visibility:   Optional<BoardPostRecordVisibility>,
  pub is_versioned:        Optional<bool>,
  pub is_enabled:          Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for UpdateBoard {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      group_id: self.group_id.into_active_value(),
      name: self.name.into_active_value(),
      description: self.description.into_active_value(),
      order: self.order.into_active_value(),
      can_upload_file: self.can_upload_file.into_active_value(),
      max_file_size: self.max_file_size.into_active_value(),
      max_file_count: self.max_file_count.into_active_value(),
      allowed_file_types: self.allowed_file_types.into_active_value(),
      reaction_visibility: self.reaction_visibility.into_active_value(),
      viewer_visibility: self.viewer_visibility.into_active_value(),
      is_versioned: self.is_versioned.into_active_value(),
      is_enabled: self.is_enabled.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardUpdated;

impl IntoResponse for BoardUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardDeleted;

impl IntoResponse for BoardDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    BoardFilter::validate().expect("the board filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: BoardFilter =
      serde_urlencoded::from_str("search=a&name=b&group_id=1").expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.name.is_some());
    assert!(filter.group_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardFilter = serde_urlencoded::from_str("group_id=1,2").expect("a list parses");
    assert_eq!(filter.group_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardFilter = serde_urlencoded::from_str("group_id=").expect("an empty value parses");
    assert!(listed(filter.group_id.as_ref()).is_none());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. The reserved `order` key that drives the display sequence cannot be a
  /// field either, and every undeclared key stays unreachable.
  #[test]
  fn the_non_field_keys_resolve_to_nothing() {
    for spelling in ["order", "description", "id", "groupId", "is_enabled"] {
      assert!(BoardFilter::field(spelling).is_none(), "{spelling}");
      assert!(BoardFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BoardFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  /// The integrated search spans two columns, so it reduces to no expression and the base has
  /// nothing to compile; its flat parameter is untouched.
  #[test]
  fn the_integrated_search_field_has_no_column() {
    let search = BoardFilter::field("search").expect("`search` is declared");

    assert!(matches!(search.strategy, MatchStrategy::Custom));
    assert!(BoardFilter::column("search").is_none());
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(BoardFilter::sort_column("name").is_some());
    assert!(BoardFilter::sort_column("created_at").is_some());
    assert!(BoardFilter::sort_column("search").is_none());
    assert!(BoardFilter::sort_column("group_id").is_none());
  }
}
