use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_topics::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{DeriveIntoActiveModel, EntityTrait, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardTopicFilter {
  pub search:   Option<String>,
  pub name:     Option<OneOrMany<String>>,
  pub board_id: Option<OneOrMany<i32>>,
}

impl BoardTopicFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(listed(self.board_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::BoardId)))
      .apply_if(listed(self.name.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::Name)))
  }
}

/// What `/board-topics` accepts, declared once.
///
/// `search` and `name` both stand on the topic name and differ only in how they match it — one
/// wraps an `ILIKE`, the other compares — which is why the strategy is declared per field rather
/// than read off the kind.
const BOARD_TOPIC_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_sort(),
  FieldSpec::new("board_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/boards", "name"),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for BoardTopicFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_TOPIC_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "search" | "name" => Column::Name.into_simple_expr(),
      "board_id" => Column::BoardId.into_simple_expr(),
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
pub struct BoardTopicListItem {
  pub id:          i32,
  pub board_id:    i32,
  pub name:        String,
  pub description: Option<String>,
  pub order:       i32,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardTopic {
  pub id:          i32,
  pub board_id:    i32,
  pub name:        String,
  pub description: Option<String>,
  pub order:       i32,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardTopic {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardTopic {
  pub id:          Optional<i32>,
  pub name:        String,
  pub description: Optional<String>,
  pub order:       Optional<i32>,
  pub board_id:    i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicCreated {
  pub id: i32,
}

impl IntoResponse for BoardTopicCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardTopic {
  pub name:        Optional<String>,
  #[serde(default)]
  pub description: Nullable<String>,
  pub order:       Optional<i32>,
  pub board_id:    Optional<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicUpdated;

impl IntoResponse for BoardTopicUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicDeleted;

impl IntoResponse for BoardTopicDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    BoardTopicFilter::validate().expect("the board topic filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: BoardTopicFilter =
      serde_urlencoded::from_str("search=a&name=b&board_id=1").expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.name.is_some());
    assert!(filter.board_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardTopicFilter =
      serde_urlencoded::from_str("name=a,b&board_id=1,2").expect("a list parses");

    assert_eq!(filter.name.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.board_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardTopicFilter =
      serde_urlencoded::from_str("name=&board_id=").expect("an empty value parses");

    assert!(listed(filter.name.as_ref()).is_none());
    assert!(listed(filter.board_id.as_ref()).is_none());
  }

  /// The read scope is one predicate emitted into the list statement, so nothing is answered by a
  /// refusal before the query and there is no scope-selecting parameter to withhold. What stays
  /// unreachable is every key the resource simply never declared.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["order", "description", "id", "createdAt"] {
      assert!(BoardTopicFilter::field(spelling).is_none(), "{spelling}");
      assert!(BoardTopicFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BoardTopicFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(BoardTopicFilter::sort_column("name").is_some());
    assert!(BoardTopicFilter::sort_column("created_at").is_some());
    // Declared, but not sortable.
    assert!(BoardTopicFilter::sort_column("search").is_none());
    assert!(BoardTopicFilter::sort_column("board_id").is_none());
  }
}
