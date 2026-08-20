use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_post_reports::Column;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoSimpleExpr};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostReportFilter {
  pub post_id: Option<OneOrMany<i32>>,
  pub user_id: Option<OneOrMany<i32>>,
}

/// What `/board-post-reports` accepts, declared once.
///
/// The route gate is the whole access decision here — the list carries no row-scope predicate — so
/// both keys narrow inside an all-or-nothing scope rather than choosing one.
const BOARD_POST_REPORT_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("post_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/board-posts", "title")
    .with_facet(),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_facet(),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
];

impl FilterSchema for BoardPostReportFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_POST_REPORT_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "post_id" => Column::PostId.into_simple_expr(),
      "user_id" => Column::UserId.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "createdAt"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReportListItem {
  pub id:         i32,
  pub post_id:    i32,
  pub user_id:    i32,
  pub reason:     String,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPostReport {
  pub id:         i32,
  pub post_id:    i32,
  pub user_id:    i32,
  pub reason:     String,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPostReport {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPostReport {
  pub post_id: i32,
  pub reason:  String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReportCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostReportCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReportDeleted;

impl IntoResponse for BoardPostReportDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::util::filter::listed;

  #[test]
  fn the_declaration_is_sound() {
    BoardPostReportFilter::validate().expect("the report filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: BoardPostReportFilter =
      serde_urlencoded::from_str("post_id=1&user_id=2").expect("the flat filter parses");

    assert!(filter.post_id.is_some());
    assert!(filter.user_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardPostReportFilter =
      serde_urlencoded::from_str("post_id=1,2&user_id=3,4").expect("a list parses");

    assert_eq!(filter.post_id.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardPostReportFilter =
      serde_urlencoded::from_str("post_id=&user_id=").expect("an empty value parses");

    assert!(listed(filter.post_id.as_ref()).is_none());
    assert!(listed(filter.user_id.as_ref()).is_none());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["reason", "id", "postId", "userId"] {
      assert!(BoardPostReportFilter::field(spelling).is_none(), "{spelling}");
      assert!(BoardPostReportFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BoardPostReportFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(BoardPostReportFilter::sort_column("createdAt").is_some());
    // Declared, but not sortable.
    assert!(BoardPostReportFilter::sort_column("post_id").is_none());
    assert!(BoardPostReportFilter::sort_column("user_id").is_none());
  }
}
