use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_post_reactions::{ActiveModel, Column};
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, IntoSimpleExpr, Set};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostReactionFilter {
  /// Which post's reactions are read. It is answered **before** the list query — a missing value is
  /// a bad request and the board's reaction visibility can refuse outright — so it chooses a scope
  /// rather than narrowing one, and it is deliberately not a declared field. Declared, its refusal
  /// would become an empty page, which tells the caller the post exists and carries no reactions.
  pub post_id: Option<i32>,
  pub user_id: Option<OneOrMany<i32>>,
  pub value:   Option<String>,
}

/// What `/board-post-reactions` accepts, declared once.
///
/// `user_id` narrows within the rows the post's visibility already opened. Under a private board
/// the list has already pinned the caller's own id, so naming it here can only narrow further.
const BOARD_POST_REACTION_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("value", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq])
    .with_sort()
    .with_facet(),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_facet(),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
];

impl FilterSchema for BoardPostReactionFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_POST_REACTION_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "value" => Column::Value.into_simple_expr(),
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
pub struct BoardPostReactionListItem {
  pub id:         i32,
  pub post_id:    i32,
  pub user_id:    Option<i32>,
  pub value:      String,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPostReaction {
  pub id:         i32,
  pub post_id:    i32,
  pub user_id:    Option<i32>,
  pub value:      String,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPostReaction {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPostReaction {
  pub id:      Optional<i32>,
  pub post_id: i32,
  pub user_id: Nullable<i32>,
  pub value:   String,
}

impl IntoActiveModel<ActiveModel> for CreateBoardPostReaction {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      post_id: Set(self.post_id),
      user_id: self.user_id.into_active_value(),
      value: Set(self.value),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostReactionCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPostReaction {
  pub post_id: Optional<i32>,
  pub user_id: Nullable<i32>,
  pub value:   Optional<String>,
}

impl IntoActiveModel<ActiveModel> for UpdateBoardPostReaction {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      post_id: self.post_id.into_active_value(),
      user_id: self.user_id.into_active_value(),
      value: self.value.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionUpdated;

impl IntoResponse for BoardPostReactionUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionDeleted;

impl IntoResponse for BoardPostReactionDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    BoardPostReactionFilter::validate().expect("the reaction filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: BoardPostReactionFilter =
      serde_urlencoded::from_str("value=like&user_id=7").expect("the flat filter parses");

    assert!(filter.value.is_some());
    assert!(filter.user_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardPostReactionFilter = serde_urlencoded::from_str("user_id=1,2").expect("a list parses");
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardPostReactionFilter =
      serde_urlencoded::from_str("user_id=").expect("an empty value parses");
    assert!(crate::util::filter::listed(filter.user_id.as_ref()).is_none());
  }

  /// `post_id` picks which post is read and is answered by a refusal before the query runs, so it
  /// is not a field. Nothing may reach it — not the tree, not the sort key, under any spelling.
  #[test]
  fn the_scope_selecting_parameter_resolves_to_nothing() {
    for spelling in ["post_id", "postId", "post"] {
      assert!(BoardPostReactionFilter::field(spelling).is_none(), "{spelling}");
      assert!(BoardPostReactionFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BoardPostReactionFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(BoardPostReactionFilter::sort_column("value").is_some());
    assert!(BoardPostReactionFilter::sort_column("createdAt").is_some());
    // Declared, but not sortable.
    assert!(BoardPostReactionFilter::sort_column("user_id").is_none());
  }
}
