//! A tree of conditions a request may carry, and its compilation into one `Condition`.
//!
//! The flat parameters express one predicate per key, all of them ANDed. A tree expresses the same
//! predicates in a shape that also says *how they combine*: a node is either a comparison on one
//! declared field or a group of nodes joined by AND or OR.
//!
//! It rides the list request as one reserved query-string key, `where`, carrying the JSON text of
//! the root node. Reading it happens in two phases, and the split is what keeps the base free of
//! any resource's business.
//!
//! **Parse** knows nothing about any resource. It reads the JSON, refuses a payload over the byte
//! cap before serde ever sees it, and walks the result once for depth, node count, list length,
//! string length and the per-request caps on the two operator families that drive a scan. Every
//! failure here is a bad request.
//!
//! **Compile** reads the resource's declaration. A leaf must name a declared key, that key's field
//! must reduce to one expression, the field must have declared the operator, and the value must
//! match the operator's arity and the field's kind. Only then does the key resolve to an
//! expression. An undeclared key is refused on the declaration alone, before anything is resolved,
//! so it can never become SQL.
//!
//! **A tree only ever narrows.** What compilation yields is a `Condition`, and a `Condition` can
//! only be added to a query that already carries the read scope. An OR among a tree's own nodes is
//! one conjunct of the WHERE, parenthesized inside it; there is no expression a tree can name that
//! reaches the scope predicate, because the scope is built over columns no declaration exposes.

use chrono::DateTime;
use error::SumidaError;
use sea_orm::sea_query::{Alias, Condition, Expr, ExprTrait, SimpleExpr};
use sea_orm::{EntityTrait, QueryFilter, Select};
use serde::{Deserialize, Deserializer};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::schema::FilterSchema;
use super::spec::{Arity, FieldKind, FieldSpec, MatchStrategy, Op};
use crate::util::db::{ends_with_ilike, starts_with_ilike, tsquery, wrap_ilike};

/// The reserved query-string key a tree rides on.
pub const CONDITION_KEY: &str = "where";

/// The decoded JSON a request may carry. The whole request line has to survive a reverse proxy,
/// and JSON is mostly punctuation that percent-encodes to three bytes each, so this is roughly
/// twice as much on the wire. Checked on the raw text, so an oversized payload never costs a parse.
pub const MAX_CONDITION_BYTES: usize = 2048;

/// Nesting, counting the root as depth 1. `(A and B) or (C and (D or E))` is depth 4 and is already
/// more structure than a form can show legibly; each level also multiplies the parenthesized
/// subplans the planner has to consider.
pub const MAX_CONDITION_DEPTH: usize = 4;

/// Groups and leaves together. Postgres plans a predicate of this size without difficulty, and a
/// form that honestly needs more rows of conditions has stopped being a form. The byte cap bounds
/// the cost of receiving a request; this one bounds the cost of planning it, so a small payload
/// cannot expand into an expensive plan. The two meet where a leaf is at its tersest: verbose
/// leaves run out of bytes first, terse ones run out of nodes first.
pub const MAX_CONDITION_NODES: usize = 32;

/// Values in one `in` / `nin` list. Larger than any realistic multi-select — the biggest declared
/// enum has eight variants — while stopping one leaf from becoming a list long enough that Postgres
/// abandons the index probe for a scan.
pub const MAX_CONDITION_VALUES: usize = 64;

/// Bytes in one string value. Longer than any phrase worth matching against a text column, and it
/// bounds both the `ILIKE` pattern a substring operator builds and the term a tsquery parses. The
/// byte cap bounds the total, but one very long term costs more than many short ones, so this is
/// the cap that protects the index.
pub const MAX_CONDITION_TEXT_LENGTH: usize = 256;

/// `matches` leaves in one request. Each is its own probe into the tsvector index, and a tree that
/// ORs many of them degenerates into a scan of the scope. Four covers the realistic "any of these
/// topics" ask. A guard picked by reasoning about the index, not a measured threshold.
pub const MAX_CONDITION_FULL_TEXT: usize = 4;

/// `contains` / `startsWith` / `endsWith` leaves in one request. Where the column carries a trigram
/// index each is a separate probe, exactly like a `matches` leaf; where it carries none each is a
/// pattern match evaluated per row of the scan the query already does. Both readings are bounded
/// here rather than only by the node cap, which would allow dozens of either.
pub const MAX_CONDITION_SUBSTRING: usize = 8;

/// One node: a group that combines its children, or a comparison on one declared field.
///
/// The tag is internal so a malformed payload reports which variant and which member failed rather
/// than collapsing into "matched no variant".
///
/// There is no negation group. The vocabulary already carries `ne`, `nin` and `isNotNull`; a
/// group-level negation over three-valued SQL changes what a NULL row means in a way no form
/// control could show honestly.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConditionNode {
  /// Every child must hold.
  All { of: Vec<ConditionNode> },
  /// Any child may hold.
  Any { of: Vec<ConditionNode> },
  /// One operator against one declared key.
  Field {
    key:   String,
    op:    Op,
    #[serde(default)]
    value: Option<JsonValue>,
  },
}

/// A parsed tree, already inside every parse-time limit. Its root is any node, so a single leaf is
/// a whole tree.
#[derive(Clone, Debug)]
pub struct ConditionTree {
  root: ConditionNode,
}

/// What the walk counts as it goes. Depth is carried down; these are carried across.
#[derive(Default)]
struct Tally {
  nodes:     usize,
  full_text: usize,
  substring: usize,
}

impl ConditionTree {
  /// Reads the JSON text of one node and enforces every limit that does not need a declaration.
  pub fn parse(raw: &str) -> Result<Self, ConditionError> {
    if raw.len() > MAX_CONDITION_BYTES {
      return Err(ConditionError::TooLarge(raw.len()));
    }

    let root: ConditionNode =
      serde_json::from_str(raw).map_err(|error| ConditionError::Malformed(error.to_string()))?;
    let tree = Self { root };
    tree.walk(&tree.root, 1, &mut Tally::default())?;

    Ok(tree)
  }

  fn walk(&self, node: &ConditionNode, depth: usize, tally: &mut Tally) -> Result<(), ConditionError> {
    if depth > MAX_CONDITION_DEPTH {
      return Err(ConditionError::TooDeep);
    }

    tally.nodes += 1;

    if tally.nodes > MAX_CONDITION_NODES {
      return Err(ConditionError::TooManyNodes);
    }

    match node {
      ConditionNode::All { of } | ConditionNode::Any { of } => {
        // A group with no children reads as always-true and as always-false, and the two differ.
        if of.is_empty() {
          return Err(ConditionError::EmptyGroup);
        }

        for child in of {
          self.walk(child, depth + 1, tally)?;
        }
      }
      ConditionNode::Field { key, op, value } => {
        if *op == Op::Matches {
          tally.full_text += 1;

          if tally.full_text > MAX_CONDITION_FULL_TEXT {
            return Err(ConditionError::TooManyFullText);
          }
        }

        if op.is_substring() {
          tally.substring += 1;

          if tally.substring > MAX_CONDITION_SUBSTRING {
            return Err(ConditionError::TooManySubstring);
          }
        }

        check_shape(key, *op, value.as_ref())?;
      }
    }

    Ok(())
  }

  /// The tree as one condition over the resource's declaration.
  ///
  /// A `Condition` is the widest thing this can produce, and adding one to a query can only
  /// narrow it.
  pub fn compile<F>(&self) -> Result<Condition, ConditionError>
  where
    F: FilterSchema,
  {
    compile_node::<F>(&self.root)
  }
}

/// The `where` parameter as a list route reads it, beside the flat filter and the pagination.
///
/// Parsing happens while the request is being extracted, so a tree that breaks a parse-time limit
/// is refused before any handler runs.
#[derive(Clone, Debug, Default)]
pub struct ConditionQuery {
  tree: Option<ConditionTree>,
}

impl ConditionQuery {
  pub fn tree(&self) -> Option<&ConditionTree> {
    self.tree.as_ref()
  }

  /// ANDs the tree into a query the caller has already scoped and flat-filtered.
  ///
  /// This is the only way the tree meets a query: one `filter` call, one condition. A request
  /// carrying no tree gets the query back untouched.
  pub fn apply<F, E>(&self, query: Select<E>) -> Result<Select<E>, SumidaError>
  where
    F: FilterSchema,
    E: EntityTrait,
  {
    match &self.tree {
      None => Ok(query),
      Some(tree) => Ok(query.filter(tree.compile::<F>()?)),
    }
  }
}

impl<'de> Deserialize<'de> for ConditionQuery {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    struct Raw {
      #[serde(default, rename = "where")]
      tree: Option<String>,
    }

    let raw = Raw::deserialize(deserializer)?;

    // An empty value is a form sending a box nobody filled in, exactly as it is for the flat
    // parameters, and it narrows nothing.
    match raw.tree.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
      None => Ok(Self { tree: None }),
      Some(text) => ConditionTree::parse(text)
        .map(|tree| Self { tree: Some(tree) })
        .map_err(serde::de::Error::custom),
    }
  }
}

/// The `where` parameter, written out for a reader of the document.
///
/// The type reads itself rather than being derived from named fields — the parameter is spelled
/// `where` and the tree arrives as one string — so the parameter is stated here, beside the reader
/// it has to agree with.
#[cfg(feature = "openapi")]
impl utoipa::IntoParams for ConditionQuery {
  fn into_params(
    parameter_in_provider: impl Fn() -> Option<utoipa::openapi::path::ParameterIn>,
  ) -> Vec<utoipa::openapi::path::Parameter> {
    use utoipa::openapi::Required;
    use utoipa::openapi::path::{ParameterBuilder, ParameterIn};
    use utoipa::openapi::schema::{ObjectBuilder, Type};

    vec![
      ParameterBuilder::new()
        .name("where")
        .parameter_in(parameter_in_provider().unwrap_or(ParameterIn::Query))
        .required(Required::False)
        .description(Some(
          "How several comparisons are grouped, as a JSON tree. Only the keys and operators this \
           list declares may appear in it, and the caps on its size are the ones `filters` states. \
           It narrows whatever the flat parameters already narrowed — sending both applies both. An \
           empty value narrows nothing.",
        ))
        .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
        .build(),
    ]
  }
}

/// Whether the value carried matches what the operator takes, and whether the pieces are inside the
/// per-value caps. Types are the field's business and are checked when the tree is compiled.
fn check_shape(key: &str, op: Op, value: Option<&JsonValue>) -> Result<(), ConditionError> {
  let present = || {
    value.ok_or_else(|| ConditionError::MissingValue {
      key: key.to_owned(),
      op,
    })
  };

  match op.arity() {
    Arity::Nullary => match value {
      None | Some(JsonValue::Null) => Ok(()),
      Some(_) => Err(ConditionError::UnexpectedValue {
        key: key.to_owned(),
        op,
      }),
    },
    Arity::One => check_text_length(key, present()?),
    Arity::Many => {
      let items = present()?.as_array().ok_or_else(|| ConditionError::NotAList {
        key: key.to_owned(),
        op,
      })?;

      // A tree node exists only because someone authored it. Dropping an authored child changes
      // what its group means — it widens an `all` and narrows an `any` — so an empty list is
      // refused rather than read as "unfiltered".
      if items.is_empty() {
        return Err(ConditionError::EmptyList {
          key: key.to_owned(),
          op,
        });
      }

      if items.len() > MAX_CONDITION_VALUES {
        return Err(ConditionError::TooManyValues {
          key:   key.to_owned(),
          count: items.len(),
        });
      }

      items.iter().try_for_each(|item| check_text_length(key, item))
    }
    Arity::Range => {
      let bounds = present()?.as_array().filter(|items| items.len() == 2).ok_or_else(|| {
        ConditionError::NotARange {
          key: key.to_owned(),
          op,
        }
      })?;

      if bounds.iter().any(JsonValue::is_null) {
        return Err(ConditionError::NotARange {
          key: key.to_owned(),
          op,
        });
      }

      bounds.iter().try_for_each(|bound| check_text_length(key, bound))
    }
  }
}

fn check_text_length(key: &str, value: &JsonValue) -> Result<(), ConditionError> {
  match value.as_str() {
    Some(text) if text.len() > MAX_CONDITION_TEXT_LENGTH => Err(ConditionError::TextTooLong {
      key:    key.to_owned(),
      length: text.len(),
    }),
    _ => Ok(()),
  }
}

fn compile_node<F>(node: &ConditionNode) -> Result<Condition, ConditionError>
where
  F: FilterSchema,
{
  match node {
    ConditionNode::All { of } => fold::<F>(Condition::all(), of),
    ConditionNode::Any { of } => fold::<F>(Condition::any(), of),
    ConditionNode::Field { key, op, value } => {
      Ok(Condition::all().add(compile_leaf::<F>(key, *op, value.as_ref())?))
    }
  }
}

fn fold<F>(mut group: Condition, children: &[ConditionNode]) -> Result<Condition, ConditionError>
where
  F: FilterSchema,
{
  for child in children {
    group = group.add(compile_node::<F>(child)?);
  }

  Ok(group)
}

/// One leaf into one expression, refusing on the declaration before anything is resolved.
fn compile_leaf<F>(key: &str, op: Op, value: Option<&JsonValue>) -> Result<SimpleExpr, ConditionError>
where
  F: FilterSchema,
{
  // The whole safety story is this line. A key the resource did not declare stops here, before the
  // resource is asked for an expression at all.
  let field = F::field(key).ok_or_else(|| ConditionError::UndeclaredKey(key.to_owned()))?;

  // A custom field's predicate spans several mechanisms and reduces to no single expression, so
  // there is nothing here to compile. The resource builds it from its own parameter.
  if matches!(field.strategy, MatchStrategy::Custom) {
    return Err(ConditionError::NotCompilable(key.to_owned()));
  }

  // What the resource *declared*, not what its kind and strategy could have meant.
  if !field.ops.contains(&op) {
    return Err(ConditionError::OpNotDeclared {
      key: key.to_owned(),
      op,
    });
  }

  let enum_type = F::enum_type(key);
  let column = F::resolve(key).ok_or_else(|| ConditionError::UndeclaredKey(key.to_owned()))?;

  match op.arity() {
    Arity::Nullary => Ok(match op {
      Op::IsNull => column.is_null(),
      _ => column.is_not_null(),
    }),
    Arity::One => {
      let value = value.expect("arity was checked when the tree was parsed");

      // The three substring operators and the full-text operator take the text itself, not a bound
      // comparison value: one builds an escaped pattern, the other a tsquery.
      if op.is_substring() || op == Op::Matches {
        let text = as_text(field, value)?;

        return Ok(match op {
          Op::Contains => wrap_ilike(column, text),
          Op::StartsWith => starts_with_ilike(column, text),
          Op::EndsWith => ends_with_ilike(column, text),
          _ => tsquery(column, text),
        });
      }

      let bound = bind(field, enum_type, value)?;

      Ok(match op {
        Op::Eq => column.eq(bound),
        Op::Ne => column.ne(bound),
        Op::Lt => ExprTrait::lt(column, bound),
        Op::Lte => ExprTrait::lte(column, bound),
        Op::Gt => ExprTrait::gt(column, bound),
        _ => ExprTrait::gte(column, bound),
      })
    }
    Arity::Many => {
      let items = value.and_then(JsonValue::as_array).expect("arity was checked when the tree was parsed");
      let bounds = items.iter().map(|item| bind(field, enum_type, item)).collect::<Result<Vec<_>, _>>()?;

      Ok(match op {
        Op::In => column.is_in(bounds),
        _ => column.is_not_in(bounds),
      })
    }
    Arity::Range => {
      let bounds = value.and_then(JsonValue::as_array).expect("arity was checked when the tree was parsed");

      Ok(column.between(bind(field, enum_type, &bounds[0])?, bind(field, enum_type, &bounds[1])?))
    }
  }
}

/// A JSON value as the field's kind says it must be. Nothing is coerced: a number where a string
/// belongs is a bad request, not a silently stringified number.
fn bind(
  field: &FieldSpec,
  enum_type: Option<&'static str>,
  value: &JsonValue,
) -> Result<SimpleExpr, ConditionError> {
  let mismatch = || ConditionError::ValueKind {
    key:  field.key.to_owned(),
    kind: field.kind,
  };

  Ok(match field.kind {
    FieldKind::Text => Expr::val(as_text(field, value)?.to_owned()).into(),
    FieldKind::Boolean => Expr::val(value.as_bool().ok_or_else(mismatch)?).into(),
    FieldKind::Integer => Expr::val(value.as_i64().ok_or_else(mismatch)?).into(),
    FieldKind::Decimal => Expr::val(value.as_f64().ok_or_else(mismatch)?).into(),
    FieldKind::Uuid => {
      let text = as_text(field, value)?;
      let id = Uuid::parse_str(text).map_err(|_| mismatch())?;

      Expr::val(id).into()
    }
    FieldKind::DateTime => {
      let text = as_text(field, value)?;
      let at = DateTime::parse_from_rfc3339(text).map_err(|_| mismatch())?;

      Expr::val(at).into()
    }
    FieldKind::Enum => {
      let text = as_text(field, value)?;

      // The same rule the flat form states: a value outside the declared set is refused, never
      // dropped. A dropped value reads as a filter that was never applied.
      if !field.variants.contains(&text) {
        return Err(ConditionError::UnknownVariant {
          key:   field.key.to_owned(),
          value: text.to_owned(),
        });
      }

      match enum_type {
        // The expression behind a key is a bare expression and has lost the column type sea-orm
        // would otherwise cast through, so the resource names the database type and the cast is
        // written here.
        Some(name) => Expr::val(text.to_owned()).as_enum(Alias::new(name)),
        None => Expr::val(text.to_owned()).into(),
      }
    }
  })
}

fn as_text<'a>(field: &FieldSpec, value: &'a JsonValue) -> Result<&'a str, ConditionError> {
  value.as_str().ok_or_else(|| ConditionError::ValueKind {
    key:  field.key.to_owned(),
    kind: field.kind,
  })
}

/// A tree that cannot be served. Every variant is something the caller sent, so every one of them
/// is a bad request — nothing here reaches a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConditionError {
  TooLarge(usize),
  Malformed(String),
  TooDeep,
  TooManyNodes,
  TooManyFullText,
  TooManySubstring,
  EmptyGroup,
  UndeclaredKey(String),
  NotCompilable(String),
  OpNotDeclared { key: String, op: Op },
  MissingValue { key: String, op: Op },
  UnexpectedValue { key: String, op: Op },
  NotAList { key: String, op: Op },
  NotARange { key: String, op: Op },
  EmptyList { key: String, op: Op },
  TooManyValues { key: String, count: usize },
  TextTooLong { key: String, length: usize },
  ValueKind { key: String, kind: FieldKind },
  UnknownVariant { key: String, value: String },
}

impl std::fmt::Display for ConditionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::TooLarge(bytes) => {
        write!(f, "`{CONDITION_KEY}` is {bytes} bytes, over the {MAX_CONDITION_BYTES} allowed")
      }
      Self::Malformed(detail) => write!(f, "`{CONDITION_KEY}` is not a readable condition: {detail}"),
      Self::TooDeep => write!(f, "conditions nest deeper than {MAX_CONDITION_DEPTH}"),
      Self::TooManyNodes => write!(f, "conditions hold more than {MAX_CONDITION_NODES} nodes"),
      Self::TooManyFullText => {
        write!(f, "conditions hold more than {MAX_CONDITION_FULL_TEXT} `matches` comparisons")
      }
      Self::TooManySubstring => {
        write!(f, "conditions hold more than {MAX_CONDITION_SUBSTRING} substring comparisons")
      }
      Self::EmptyGroup => write!(f, "a condition group has no children"),
      Self::UndeclaredKey(key) => write!(f, "`{key}` is not a field of this list"),
      Self::NotCompilable(key) => write!(f, "`{key}` cannot be used in a condition tree"),
      Self::OpNotDeclared { key, op } => write!(f, "`{key}` does not take `{op}`"),
      Self::MissingValue { key, op } => write!(f, "`{key}` `{op}` needs a value"),
      Self::UnexpectedValue { key, op } => write!(f, "`{key}` `{op}` takes no value"),
      Self::NotAList { key, op } => write!(f, "`{key}` `{op}` needs a list of values"),
      Self::NotARange { key, op } => write!(f, "`{key}` `{op}` needs a lower and an upper bound"),
      Self::EmptyList { key, op } => write!(f, "`{key}` `{op}` was given an empty list"),
      Self::TooManyValues { key, count } => {
        write!(f, "`{key}` was given {count} values, over the {MAX_CONDITION_VALUES} allowed")
      }
      Self::TextTooLong { key, length } => write!(
        f,
        "`{key}` was given {length} bytes of text, over the {MAX_CONDITION_TEXT_LENGTH} allowed"
      ),
      Self::ValueKind { key, kind } => write!(f, "`{key}` takes a {kind:?} value"),
      Self::UnknownVariant { key, value } => write!(f, "`{key}` does not take the value `{value}`"),
    }
  }
}

impl std::error::Error for ConditionError {}

impl From<ConditionError> for SumidaError {
  fn from(error: ConditionError) -> Self {
    SumidaError::BadRequest(error.to_string())
  }
}

#[cfg(test)]
mod tests {
  use sea_orm::sea_query::{PostgresQueryBuilder, Query};

  use super::*;

  struct Sample;

  impl FilterSchema for Sample {
    fn fields() -> &'static [FieldSpec] {
      const FIELDS: &[FieldSpec] = &[
        FieldSpec::new(
          "title",
          FieldKind::Text,
          MatchStrategy::Substring,
          &[Op::Contains, Op::StartsWith, Op::EndsWith, Op::Eq],
        )
        .with_sort(),
        FieldSpec::new("body", FieldKind::Text, MatchStrategy::FullText, &[Op::Matches]),
        FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Matches]),
        FieldSpec::new("status", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
          .with_variants(&["draft", "published"]),
        FieldSpec::new(
          "views",
          FieldKind::Integer,
          MatchStrategy::Comparison,
          &[Op::Eq, Op::Between, Op::Gte, Op::IsNull],
        )
        .with_sort(),
      ];

      FIELDS
    }

    fn column(key: &str) -> Option<SimpleExpr> {
      match key {
        "title" | "body" | "status" | "views" => Some(Expr::col(Alias::new(key)).into()),
        _ => None,
      }
    }

    fn enum_type(key: &str) -> Option<&'static str> {
      match key {
        "status" => Some("sample_status_enum"),
        _ => None,
      }
    }

    fn default_sort() -> &'static str {
      "views"
    }
  }

  /// Builds the statement the way a list route does: the scope predicate first, the compiled tree
  /// added afterwards through the same `Condition`-only entry point sea-orm's `filter` uses.
  fn sql(raw: &str) -> String {
    let condition = ConditionTree::parse(raw).expect("the tree parses").compile::<Sample>();
    let mut statement = Query::select();
    statement
      .expr(Expr::val(1))
      .cond_where(Expr::col(Alias::new("owner")).eq("me"))
      .cond_where(condition.expect("the tree compiles"));

    statement.to_string(PostgresQueryBuilder)
  }

  fn refuse(raw: &str) -> ConditionError {
    match ConditionTree::parse(raw) {
      Err(error) => error,
      Ok(tree) => tree.compile::<Sample>().expect_err("the tree must be refused"),
    }
  }

  #[test]
  fn an_undeclared_key_is_refused_before_anything_resolves() {
    assert_eq!(
      refuse(r#"{"type":"field","key":"password","op":"eq","value":"x"}"#),
      ConditionError::UndeclaredKey("password".into())
    );
  }

  #[test]
  fn an_operator_the_field_did_not_declare_is_refused() {
    assert_eq!(
      refuse(r#"{"type":"field","key":"title","op":"in","value":["a"]}"#),
      ConditionError::OpNotDeclared {
        key: "title".into(),
        op:  Op::In,
      }
    );
  }

  #[test]
  fn a_custom_field_has_nothing_to_compile() {
    assert_eq!(
      refuse(r#"{"type":"field","key":"search","op":"matches","value":"a"}"#),
      ConditionError::NotCompilable("search".into())
    );
  }

  #[test]
  fn a_full_text_field_lowers_to_its_own_query_and_to_nothing_else() {
    let statement = sql(r#"{"type":"field","key":"body","op":"matches","value":"rust"}"#);

    assert!(statement.contains("plainto_tsquery"), "{statement}");
    assert!(!statement.to_uppercase().contains("ILIKE"), "{statement}");

    // The substring operators are not in its declaration, so there is no way to ask for one.
    assert_eq!(
      refuse(r#"{"type":"field","key":"body","op":"contains","value":"rust"}"#),
      ConditionError::OpNotDeclared {
        key: "body".into(),
        op:  Op::Contains,
      }
    );
  }

  #[test]
  fn the_three_substring_operators_build_the_three_patterns() {
    assert!(sql(r#"{"type":"field","key":"title","op":"contains","value":"a"}"#).contains("'%a%'"));
    assert!(sql(r#"{"type":"field","key":"title","op":"startsWith","value":"a"}"#).contains("'a%'"));
    assert!(sql(r#"{"type":"field","key":"title","op":"endsWith","value":"a"}"#).contains("'%a'"));
  }

  #[test]
  fn an_enum_value_is_cast_to_the_type_the_resource_named() {
    let statement = sql(r#"{"type":"field","key":"status","op":"eq","value":"draft"}"#);

    assert!(statement.contains(r#"CAST('draft' AS "sample_status_enum")"#), "{statement}");

    let statement = sql(r#"{"type":"field","key":"status","op":"in","value":["draft","published"]}"#);

    assert!(statement.contains(r#"CAST('published' AS "sample_status_enum")"#), "{statement}");
  }

  #[test]
  fn an_undeclared_variant_is_refused() {
    assert_eq!(
      refuse(r#"{"type":"field","key":"status","op":"eq","value":"shredded"}"#),
      ConditionError::UnknownVariant {
        key:   "status".into(),
        value: "shredded".into(),
      }
    );
  }

  #[test]
  fn a_value_of_the_wrong_kind_is_refused() {
    assert_eq!(
      refuse(r#"{"type":"field","key":"views","op":"eq","value":"12"}"#),
      ConditionError::ValueKind {
        key:  "views".into(),
        kind: FieldKind::Integer,
      }
    );
  }

  /// Whether every `OR` in the WHERE clause sits inside parentheses. One at the top level would be
  /// an alternative to the scope predicate rather than a conjunct of it.
  fn every_or_is_nested(statement: &str) -> bool {
    let clause = statement.split_once(" WHERE ").expect("a WHERE clause").1;
    let mut depth = 0usize;

    for (offset, character) in clause.char_indices() {
      match character {
        '(' => depth += 1,
        ')' => depth = depth.saturating_sub(1),
        _ => {}
      }

      if depth == 0 && clause[offset..].starts_with(" OR ") {
        return false;
      }
    }

    true
  }

  #[test]
  fn an_or_inside_the_tree_cannot_reach_the_scope_predicate() {
    let statement = sql(
      r#"{"type":"any","of":[
        {"type":"field","key":"views","op":"gte","value":10},
        {"type":"field","key":"title","op":"contains","value":"a"}
      ]}"#,
    );

    assert!(statement.contains(" OR "), "the tree must have produced an OR: {statement}");
    assert!(
      statement.contains(r#"WHERE "owner" = 'me' AND ("#),
      "the scope must stay a conjunct: {statement}"
    );
    assert!(every_or_is_nested(&statement), "an OR escaped its group: {statement}");
  }

  #[test]
  fn a_tree_of_groups_keeps_every_or_inside_the_scope() {
    let statement = sql(
      r#"{"type":"any","of":[
        {"type":"all","of":[
          {"type":"field","key":"status","op":"eq","value":"draft"},
          {"type":"field","key":"views","op":"between","value":[1,9]}
        ]},
        {"type":"any","of":[
          {"type":"field","key":"title","op":"startsWith","value":"a"},
          {"type":"field","key":"views","op":"isNull"}
        ]}
      ]}"#,
    );

    assert!(statement.contains(r#"WHERE "owner" = 'me' AND ("#), "{statement}");
    assert!(every_or_is_nested(&statement), "an OR escaped its group: {statement}");
  }

  #[test]
  fn every_parse_time_limit_is_enforced() {
    let long = "x".repeat(MAX_CONDITION_TEXT_LENGTH + 1);

    let oversized = format!(
      r#"{{"type":"field","key":"title","op":"eq","value":"{}"}}"#,
      "y".repeat(MAX_CONDITION_BYTES)
    );
    assert!(matches!(ConditionTree::parse(&oversized), Err(ConditionError::TooLarge(_))));

    assert_eq!(
      refuse(&format!(r#"{{"type":"field","key":"title","op":"eq","value":"{long}"}}"#)),
      ConditionError::TextTooLong {
        key:    "title".into(),
        length: MAX_CONDITION_TEXT_LENGTH + 1,
      }
    );

    let mut deep = r#"{"type":"field","key":"views","op":"eq","value":1}"#.to_owned();
    for _ in 0..MAX_CONDITION_DEPTH {
      deep = format!(r#"{{"type":"all","of":[{deep}]}}"#);
    }
    assert_eq!(refuse(&deep), ConditionError::TooDeep);

    let leaves = (0..=MAX_CONDITION_NODES)
      .map(|_| r#"{"type":"field","key":"views","op":"eq","value":1}"#)
      .collect::<Vec<_>>()
      .join(",");
    assert_eq!(refuse(&format!(r#"{{"type":"all","of":[{leaves}]}}"#)), ConditionError::TooManyNodes);

    let values = (0..=MAX_CONDITION_VALUES).map(|_| "\"draft\"").collect::<Vec<_>>().join(",");
    assert_eq!(
      refuse(&format!(r#"{{"type":"field","key":"status","op":"in","value":[{values}]}}"#)),
      ConditionError::TooManyValues {
        key:   "status".into(),
        count: MAX_CONDITION_VALUES + 1,
      }
    );

    let matches = (0..=MAX_CONDITION_FULL_TEXT)
      .map(|_| r#"{"type":"field","key":"body","op":"matches","value":"a"}"#)
      .collect::<Vec<_>>()
      .join(",");
    assert_eq!(
      refuse(&format!(r#"{{"type":"any","of":[{matches}]}}"#)),
      ConditionError::TooManyFullText
    );

    let substrings = (0..=MAX_CONDITION_SUBSTRING)
      .map(|_| r#"{"type":"field","key":"title","op":"contains","value":"a"}"#)
      .collect::<Vec<_>>()
      .join(",");
    assert_eq!(
      refuse(&format!(r#"{{"type":"any","of":[{substrings}]}}"#)),
      ConditionError::TooManySubstring
    );
  }

  #[test]
  fn a_group_with_no_children_and_a_list_with_no_values_are_refused() {
    assert_eq!(refuse(r#"{"type":"all","of":[]}"#), ConditionError::EmptyGroup);
    assert_eq!(
      refuse(r#"{"type":"field","key":"status","op":"in","value":[]}"#),
      ConditionError::EmptyList {
        key: "status".into(),
        op:  Op::In,
      }
    );
  }

  #[test]
  fn the_value_has_to_match_the_operators_arity() {
    assert_eq!(
      refuse(r#"{"type":"field","key":"views","op":"isNull","value":1}"#),
      ConditionError::UnexpectedValue {
        key: "views".into(),
        op:  Op::IsNull,
      }
    );
    assert_eq!(
      refuse(r#"{"type":"field","key":"views","op":"eq"}"#),
      ConditionError::MissingValue {
        key: "views".into(),
        op:  Op::Eq,
      }
    );
    assert_eq!(
      refuse(r#"{"type":"field","key":"views","op":"between","value":[1]}"#),
      ConditionError::NotARange {
        key: "views".into(),
        op:  Op::Between,
      }
    );
    assert_eq!(
      refuse(r#"{"type":"field","key":"views","op":"between","value":[1,null]}"#),
      ConditionError::NotARange {
        key: "views".into(),
        op:  Op::Between,
      }
    );
  }

  #[test]
  fn an_absent_or_empty_parameter_carries_no_tree() {
    let query: ConditionQuery = serde_urlencoded::from_str("").expect("no parameter parses");
    assert!(query.tree().is_none());

    let query: ConditionQuery = serde_urlencoded::from_str("where=").expect("an empty value parses");
    assert!(query.tree().is_none());

    let query: ConditionQuery =
      serde_urlencoded::from_str(r#"where=%7B%22type%22%3A%22field%22%2C%22key%22%3A%22views%22%2C%22op%22%3A%22eq%22%2C%22value%22%3A1%7D"#)
        .expect("a percent-encoded tree parses");
    assert!(query.tree().is_some());
  }

  #[test]
  fn a_tree_that_breaks_a_limit_is_refused_while_the_request_is_read() {
    assert!(
      serde_urlencoded::from_str::<ConditionQuery>(
        "where=%7B%22type%22%3A%22all%22%2C%22of%22%3A%5B%5D%7D"
      )
      .is_err()
    );
    assert!(serde_urlencoded::from_str::<ConditionQuery>("where=not-json").is_err());
  }
}
