use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::mail_forms::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::Optional;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MailFormFilter {
  pub sender:  Option<String>,
  pub subject: Option<String>,
}

/// What `/mail-forms` accepts, declared once. `kind` is the form's primary key and the order the
/// page reads in, but it is not offered as a filter: the screen picks a form by following its link.
const MAIL_FORM_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("sender", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("subject", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::sort_only("kind", FieldKind::Text),
];

impl FilterSchema for MailFormFilter {
  fn fields() -> &'static [FieldSpec] {
    MAIL_FORM_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "sender" => Column::Sender.into_simple_expr(),
      "subject" => Column::Subject.into_simple_expr(),
      "kind" => Column::Kind.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "kind"
  }
}

impl MailFormFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.sender.as_ref(), |q, v| q.filter(wrap_ilike(Column::Sender.into_expr(), v)))
      .apply_if(self.subject.as_ref(), |q, v| q.filter(wrap_ilike(Column::Subject.into_expr(), v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct MailFormListItem {
  pub kind:    String,
  pub sender:  String,
  pub subject: String,

  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetMailForm {
  pub kind:    String,
  pub sender:  String,
  pub subject: String,
  pub content: String,

  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetMailForm {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateMailForm {
  pub kind:    String,
  pub sender:  String,
  pub subject: String,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFormCreated {
  pub kind: String,
}

impl IntoResponse for MailFormCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMailForm {
  pub kind:    Optional<String>,
  pub sender:  Optional<String>,
  pub subject: Optional<String>,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFormUpdated;

impl IntoResponse for MailFormUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFormDeleted;

impl IntoResponse for MailFormDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    MailFormFilter::validate().expect("the mail form filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: MailFormFilter =
      serde_urlencoded::from_str("sender=a&subject=b").expect("the flat filter parses");

    assert!(filter.sender.is_some());
    assert!(filter.subject.is_some());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["content", "created_at", "updated_at", "createdAt"] {
      assert!(MailFormFilter::field(spelling).is_none(), "{spelling}");
      assert!(MailFormFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(MailFormFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["sender", "subject", "kind"] {
      assert!(MailFormFilter::sort_column(key).is_some(), "{key}");
    }
  }

  /// One caller for the rule compiler. Which tuples it would match is never read — what these
  /// assert is the shape of the statement, not the rows it returns.
  fn caller() -> SubjectSet {
    SubjectSet::from_parts(vec!["user:7".to_owned(), "role:3#member".to_owned()], vec![7])
  }

  /// The read scope the list is built from. The projection and the joins the service adds are left
  /// out on purpose: what §11 turns on is how the scope and the tree meet in the `WHERE` clause.
  fn scope() -> Select<Entity> {
    Authz::filter(super::super::RESOURCE, "view", &caller(), Entity::find())
      .expect("the read scope compiles")
  }

  fn where_of(query: Select<Entity>) -> String {
    use sea_orm::QueryTrait;
    use sea_orm::sea_query::PostgresQueryBuilder;

    let sql = query.as_query().to_string(PostgresQueryBuilder);

    sql.split_once(" WHERE ").expect("a WHERE clause").1.to_owned()
  }

  /// The tree as the one condition a list adds to a query it has already scoped.
  fn compiled(raw: &str) -> Condition {
    ConditionTree::parse(raw)
      .expect("the tree parses")
      .compile::<MailFormFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"sender","op":"contains","value":"noreply"},
      {"type":"field","key":"subject","op":"contains","value":"welcome"}
    ]}"#;

    let bare = where_of(Entity::find().filter(compiled(TREE)));
    let scoped = where_of(scope().filter(compiled(TREE)));

    assert!(bare.contains(" OR "), "{bare}");
    assert!(scoped.contains(&where_of(scope())), "the scope must survive whole: {scoped}");
    assert!(
      scoped.ends_with(&format!(" AND ({bare})")),
      "the tree must be one more conjunct: {scoped}"
    );
  }
}
