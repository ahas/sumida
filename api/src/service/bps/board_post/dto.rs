use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_posts::*;
use entity::bps::boards::BoardPostContentVisibility;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{Condition, EntityTrait, FromQueryResult, IntoActiveValue, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use super::util;
use crate::service::auth::helper::verify_password;
use crate::service::prelude::BoardFileListItem;
use crate::util::db::{tsquery, wrap_ilike};
use crate::util::filter::{ConditionTree, FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

pub trait Reply {
  fn get_id(&self) -> i32;
  fn get_parent_id(&self) -> Option<i32>;
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct BoardPostFilter {
  pub search: Option<String>,
  pub title:  Option<String>,
  pub text:   Option<String>,
  pub kind:   Option<BoardPostKind>,
  pub status: Option<OneOrMany<BoardPostStatus>>,

  // FTS
  pub with_snippet:          bool,
  pub snippet_start_sel:     String,
  pub snippet_stop_sel:      String,
  pub snippet_max_words:     usize,
  pub snippet_min_words:     usize,
  pub snippet_max_fragments: usize,
  pub snippet_short_word:    usize,
  pub snippet_offset:        usize,
  pub snippet_length:        Option<usize>,
  pub snippet_highlight_all: bool,

  // Foreign Keys
  pub topic_id:  Option<OneOrMany<i32>>,
  pub board_id:  Option<OneOrMany<i32>>,
  pub parent_id: Option<OneOrMany<i32>>,
}

impl Default for BoardPostFilter {
  fn default() -> Self {
    Self {
      search:                None,
      title:                 None,
      text:                  None,
      kind:                  None,
      status:                None,
      // FTS
      with_snippet:          false,
      snippet_offset:        0,
      snippet_length:        None,
      snippet_start_sel:     "<mark>".into(),
      snippet_stop_sel:      "</mark>".into(),
      snippet_max_fragments: 1,
      snippet_max_words:     20,
      snippet_min_words:     5,
      snippet_short_word:    3,
      snippet_highlight_all: false,
      // Foreign Keys
      topic_id:              None,
      board_id:              None,
      parent_id:             None,
    }
  }
}

impl BoardPostFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Column::Title.into_expr(), v))
            .add(tsquery(Column::TextTsv.into_simple_expr(), v)),
        )
      })
      .apply_if(self.title.as_ref(), |q, v| q.filter(wrap_ilike(Column::Title.into_expr(), v)))
      .apply_if(self.text.as_ref(), |q, v| q.filter(tsquery(Column::TextTsv.into_simple_expr(), v)))
      .apply_if(listed(self.topic_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::TopicId)))
      .apply_if(listed(self.board_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::BoardId)))
      .apply_if(listed(self.parent_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::ParentId)))
      .apply_if(self.kind.as_ref(), |q, v| q.filter(Column::Kind.eq(v.clone().into_value())))
      .apply_if(listed(self.status.as_ref()), |q, v| {
        q.filter(Column::Status.is_in(v.iter().map(|s| s.clone().into_value())))
      })
  }
}

/// What `/board-posts` accepts, declared once.
///
/// The keys are the query-string names the list already reads, so the descriptor names exactly what
/// a caller may send. Two of them are full text — `text` runs against the post's tsvector, and
/// `search` widens that to the title as well — and both say so, because neither can be served by a
/// substring `ILIKE` over the same column.
const BOARD_POST_FIELDS: &[FieldSpec] = &[
  // Title `ILIKE` unioned with a tsquery over the body: two mechanisms in one predicate, which is
  // what `Custom` is for.
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Matches]),
  FieldSpec::new("title", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("text", FieldKind::Text, MatchStrategy::FullText, &[Op::Matches]),
  FieldSpec::new("kind", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq])
    .with_variants(&["notice", "article", "comment", "reply"])
    .with_facet(),
  FieldSpec::new("status", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["draft", "published", "archived", "deleted", "scheduled", "suspended"])
    .with_facet(),
  FieldSpec::new("board_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/boards", "name")
    .with_facet(),
  FieldSpec::new("topic_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/board-topics", "name")
    .with_facet(),
  FieldSpec::new("parent_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/board-posts", "title")
    .with_facet(),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for BoardPostFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_POST_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "title" => Column::Title.into_simple_expr(),
      // The tsvector, not the body: full text matches what the index holds.
      "text" => Column::TextTsv.into_simple_expr(),
      "kind" => Column::Kind.into_simple_expr(),
      "status" => Column::Status.into_simple_expr(),
      "board_id" => Column::BoardId.into_simple_expr(),
      "topic_id" => Column::TopicId.into_simple_expr(),
      "parent_id" => Column::ParentId.into_simple_expr(),
      "created_at" => Column::CreatedAt.into_simple_expr(),
      // `search` spans the title and the body, so it has no single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "created_at"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBoardPost {
  #[serde(default)]
  pub password: Option<String>,
}

impl ViewBoardPost {
  pub fn verify_password(&self, post_password: &str) -> bool {
    if let Some(ref view_password) = self.password {
      verify_password(view_password, post_password)
    } else {
      true
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostViewerItem {
  pub user_id:   Option<i32>,
  pub username:  Option<String>,
  pub name:      Option<String>,
  pub viewed_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostUser {
  #[sea_orm(from_alias = "user_id")]
  pub id:       i32,
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub name:     String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostBoard {
  #[sea_orm(from_alias = "board_id")]
  pub id:       i32,
  #[sea_orm(from_alias = "board_name")]
  pub name:     String,
  #[sea_orm(from_alias = "board_group_id")]
  pub group_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostTopic {
  #[sea_orm(from_alias = "topic_id")]
  pub id:   i32,
  #[sea_orm(from_alias = "topic_name")]
  pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostCommon {
  pub id:           i32,
  pub parent_id:    Option<i32>,
  pub title:        String,
  pub kind:         BoardPostKind,
  pub status:       BoardPostStatus,
  pub content_type: BoardPostContentType,
  #[serde(default)]
  pub reivision:    Nullable<i32>,
  pub tags:         Option<Vec<String>>,
  pub views:        i32,
  pub reactions:    Option<JsonValue>,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub scheduled_at: Option<DateTimeWithTimeZone>,
  pub created_at:   DateTimeWithTimeZone,
  pub updated_at:   DateTimeWithTimeZone,

  // Nested Data
  #[sea_orm(nested)]
  pub board: BoardPostBoard,
  #[sea_orm(nested, nullable)]
  pub topic: Option<BoardPostTopic>,
  #[sea_orm(nested, nullable)]
  pub user:  Option<BoardPostUser>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostListItem {
  #[sea_orm(nested)]
  #[serde(flatten)]
  pub common: BoardPostCommon,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub content: Option<String>,

  #[sea_orm(skip)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub replies: Option<Vec<BoardPostListItem>>,

  pub snippet: Option<String>,
}

impl Reply for BoardPostListItem {
  fn get_id(&self) -> i32 {
    self.common.id
  }

  fn get_parent_id(&self) -> Option<i32> {
    self.common.parent_id
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPost {
  #[sea_orm(nested)]
  #[serde(flatten)]
  pub common: BoardPostCommon,

  pub content:          Option<String>,
  pub compression_type: Option<String>,

  #[sea_orm(skip)]
  pub attached_files: Vec<BoardFileListItem>,

  #[sea_orm(skip)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub comments: Option<Vec<GetBoardPost>>,

  #[sea_orm(skip)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub replies: Option<Vec<GetBoardPost>>,
}

impl Reply for GetBoardPost {
  fn get_id(&self) -> i32 {
    self.common.id
  }

  fn get_parent_id(&self) -> Option<i32> {
    self.common.parent_id
  }
}

impl IntoResponse for GetBoardPost {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPost {
  pub id:                  Optional<i32>,
  pub board_id:            i32,
  #[serde(default)]
  pub topic_id:            Nullable<i32>,
  #[serde(default)]
  pub parent_id:           Nullable<i32>,
  pub title:               String,
  #[serde(default)]
  pub password:            Nullable<String>,
  #[serde(default)]
  pub content:             Nullable<String>,
  pub content_type:        BoardPostContentType,
  pub content_visibility:  Optional<BoardPostContentVisibility>,
  pub kind:                Optional<BoardPostKind>,
  pub status:              Optional<BoardPostStatus>,
  #[serde(default)]
  pub published_at:        Nullable<DateTimeWithTimeZone>,
  #[serde(default)]
  pub tags:                Nullable<Vec<String>>,
  pub attached_file_uuids: Option<Vec<Uuid>>,
}

impl CreateBoardPost {
  pub fn into_active_model(self) -> entity::bps::board_posts::ActiveModel {
    entity::bps::board_posts::ActiveModel {
      id: self.id.into_active_value(),
      board_id: Set(self.board_id),
      topic_id: self.topic_id.into_active_value(),
      parent_id: self.parent_id.into_active_value(),
      title: Set(self.title),
      content: self.content.into_active_value(),
      content_type: Set(self.content_type),
      content_visibility: self.content_visibility.into_active_value(),
      kind: self.kind.into_active_value(),
      status: self.status.into_active_value(),
      published_at: self.published_at.into_active_value(),
      tags: self.tags.into_active_value(),
      ..Default::default()
    }
  }

  pub fn parse_plain_text(&self) -> Result<String, SumidaError> {
    util::parse_plain_text(self.content_type.clone(), self.content.clone())
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPost {
  #[serde(default)]
  pub topic_id:            Nullable<i32>,
  pub title:               Optional<String>,
  #[serde(default)]
  pub content:             Nullable<String>,
  pub content_visibility:  Optional<BoardPostContentVisibility>,
  #[serde(default)]
  pub password:            Nullable<String>,
  pub kind:                Optional<BoardPostKind>,
  pub status:              Optional<BoardPostStatus>,
  #[serde(default)]
  pub published_at:        Nullable<DateTimeWithTimeZone>,
  #[serde(default)]
  pub tags:                Nullable<Vec<String>>,
  pub attached_file_uuids: Option<Vec<Uuid>>,
  pub detached_file_uuids: Option<Vec<Uuid>>,
}

impl UpdateBoardPost {
  pub fn into_active_model(self) -> entity::bps::board_posts::ActiveModel {
    entity::bps::board_posts::ActiveModel {
      topic_id: self.topic_id.into_active_value(),
      title: self.title.into_active_value(),
      content: self.content.into_active_value(),
      content_visibility: self.content_visibility.into_active_value(),
      kind: self.kind.into_active_value(),
      status: self.status.into_active_value(),
      published_at: self.published_at.into_active_value(),
      tags: self.tags.into_active_value(),
      ..Default::default()
    }
  }

  pub fn parse_plain_text(&self, content_type: BoardPostContentType) -> Result<String, SumidaError> {
    util::parse_plain_text(content_type, self.content.clone())
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostUpdated;

impl IntoResponse for BoardPostUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostDeleted;

impl IntoResponse for BoardPostDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostPublished {
  pub success: bool,
}

impl IntoResponse for BoardPostPublished {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    BoardPostFilter::validate().expect("the board post filter declaration must be servable");
  }

  #[test]
  fn every_declared_key_is_a_query_parameter_the_list_reads() {
    let query = "search=a&title=b&text=c&kind=notice&status=draft&board_id=1&topic_id=2&parent_id=3";
    let filter: BoardPostFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.title.is_some());
    assert!(filter.text.is_some());
    assert!(filter.kind.is_some());
    assert!(filter.status.is_some());
    assert!(filter.board_id.is_some());
    assert!(filter.topic_id.is_some());
    assert!(filter.parent_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardPostFilter =
      serde_urlencoded::from_str("status=draft,published&board_id=1,2").expect("a list parses");

    assert_eq!(filter.status.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.board_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardPostFilter =
      serde_urlencoded::from_str("status=&board_id=").expect("an empty value parses");

    assert!(listed(filter.status.as_ref()).is_none());
    assert!(listed(filter.board_id.as_ref()).is_none());
  }

  #[test]
  fn an_unknown_enum_variant_is_refused() {
    assert!(serde_urlencoded::from_str::<BoardPostFilter>("status=shredded").is_err());
    assert!(serde_urlencoded::from_str::<BoardPostFilter>("status=draft,shredded").is_err());
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(BoardPostFilter::sort_column("title").is_some());
    assert!(BoardPostFilter::sort_column("created_at").is_some());
    // Declared, but not sortable.
    assert!(BoardPostFilter::sort_column("status").is_none());
    // Not declared at all — a password column cannot be reached by naming it.
    assert!(BoardPostFilter::sort_column("password").is_none());
    assert!(BoardPostFilter::resolve("password").is_none());
  }

  /// The list query as the service builds it: the read scope first, the tree added afterwards.
  fn scoped_sql(raw: &str) -> String {
    use sea_orm::sea_query::PostgresQueryBuilder;
    use sea_orm::{QueryFilter, QueryTrait};

    let tree = ConditionTree::parse(raw).expect("the tree parses");

    Entity::find()
      .filter(Column::DeletedAt.is_null())
      .filter(tree.compile::<BoardPostFilter>().expect("the tree compiles"))
      .as_query()
      .to_string(PostgresQueryBuilder)
  }

  #[test]
  fn a_tree_reaches_the_tsvector_and_never_a_substring_scan() {
    let sql = scoped_sql(r#"{"type":"field","key":"text","op":"matches","value":"rust"}"#);

    assert!(sql.contains("plainto_tsquery"), "{sql}");
    assert!(!sql.to_uppercase().contains("ILIKE"), "{sql}");
  }

  #[test]
  fn the_integrated_search_field_is_refused_from_a_tree() {
    // It spans a title `ILIKE` unioned with a body tsquery, so it reduces to no single expression.
    // Its flat parameter is untouched; only the tree refuses it.
    let tree = ConditionTree::parse(r#"{"type":"field","key":"search","op":"matches","value":"a"}"#)
      .expect("the tree parses");

    assert!(tree.compile::<BoardPostFilter>().is_err());
  }

  #[test]
  fn an_undeclared_key_and_an_undeclared_operator_are_refused() {
    for raw in [
      r#"{"type":"field","key":"password","op":"eq","value":"x"}"#,
      r#"{"type":"field","key":"title","op":"matches","value":"x"}"#,
    ] {
      let tree = ConditionTree::parse(raw).expect("the tree parses");
      assert!(tree.compile::<BoardPostFilter>().is_err(), "{raw}");
    }
  }

  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    let sql = scoped_sql(
      r#"{"type":"any","of":[
        {"type":"field","key":"title","op":"contains","value":"rust"},
        {"type":"field","key":"status","op":"in","value":["draft","published"]}
      ]}"#,
    );

    assert!(sql.contains("IS NULL AND ("), "the scope must stay a conjunct: {sql}");
    assert!(sql.contains(" OR "), "{sql}");
  }

  /// The status column stores text, so a bound value needs no cast. A resource whose column is a
  /// database enum names its type instead.
  #[test]
  fn a_text_backed_enum_binds_without_a_cast() {
    let sql = scoped_sql(r#"{"type":"field","key":"status","op":"eq","value":"draft"}"#);
    let predicate = sql.split_once(" WHERE ").expect("a WHERE clause").1;

    assert!(BoardPostFilter::enum_type("status").is_none());
    assert!(predicate.contains("'draft'"), "{predicate}");
    assert!(!predicate.contains("CAST("), "{predicate}");
  }

  #[test]
  fn the_full_text_fields_are_declared_as_such() {
    let text = BoardPostFilter::field("text").expect("`text` is declared");

    assert!(matches!(text.strategy, MatchStrategy::FullText));
    assert!(!text.allows(Op::Contains));

    let search = BoardPostFilter::field("search").expect("`search` is declared");

    assert!(matches!(search.strategy, MatchStrategy::Custom));
    assert!(BoardPostFilter::column("search").is_none());
  }
}
