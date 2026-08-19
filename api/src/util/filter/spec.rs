//! The vocabulary a resource declares its filterable surface in.
//!
//! A declaration is data, not code: a `&'static [FieldSpec]` that a resource writes once next to its
//! `Filter` struct. Sort-key resolution and the descriptor endpoint read the same list, so a field
//! that a client can order by is a field the client was told about, by construction.
//!
//! Two things are deliberately *declared* rather than inferred.
//!
//! `MatchStrategy` is not derived from `FieldKind`. Two matching mechanisms already exist — a
//! wrapped `ILIKE` and a `tsquery` — and a text column is eligible for either. Deriving the strategy
//! from the kind would silently lower a full-text field to a substring scan over a column that has
//! no index for it, and the result would still look like a working filter.
//!
//! `RelationHint` is what makes an integer key relation-ish. The kind stays `Integer`; the hint says
//! the value points at another resource, which is what narrows the operator vocabulary to the
//! identity set and what lets a client render a picker instead of a number box.

use serde::{Deserialize, Serialize};

/// Query-string keys the list envelope already owns. A field may not claim one: `?sort=` would then
/// mean two things at once, and nothing in the request says which one the caller meant.
pub const RESERVED_KEYS: &[&str] =
  &["page", "limit", "sort", "order", "filter", "count", "where"];

/// What a value *is*. It fixes the operator vocabulary and tells a client which input to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
  Text,
  Integer,
  Decimal,
  Boolean,
  Enum,
  DateTime,
  Uuid,
}

/// How a value is *matched*. First-class, never inferred from the kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchStrategy {
  /// Ordinary SQL comparison against a column.
  Comparison,
  /// Wrapped, wildcard-escaped `ILIKE`.
  Substring,
  /// Full-text search against a tsvector. Never lowered to a substring `ILIKE`.
  FullText,
  /// The resource supplies the expression itself, because the field does not reduce to one column
  /// under one operator.
  Custom,
}

/// The operator vocabulary. Which of these a field accepts is declared per field and checked
/// against what its kind and strategy can mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Op {
  Eq,
  Ne,
  Lt,
  Lte,
  Gt,
  Gte,
  Between,
  Contains,
  StartsWith,
  EndsWith,
  In,
  Nin,
  /// The full-text operator. The only one a `FullText` field accepts.
  Matches,
  IsNull,
  IsNotNull,
}

/// How many values an operator takes. A client renders one input, two, or a multi-select from this;
/// a compiler reads it to know how many operands to expect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Arity {
  /// The operator is the whole predicate — `isNull`, `isNotNull`.
  Nullary,
  One,
  /// A list of values.
  Many,
  /// A lower and an upper bound.
  Range,
}

/// One operator and how many operands it takes, as the descriptor serves it.
///
/// Keyed by operator and nothing else — the same fifteen for every resource, because arity is a
/// property of the operator. What a *field* accepts is the field's own declaration.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorSpec {
  pub op:    Op,
  pub arity: Arity,
}

impl OperatorSpec {
  const fn of(op: Op) -> Self {
    Self {
      op,
      arity: op.arity(),
    }
  }
}

/// The whole operator vocabulary. A client reads it to know how many inputs an operator needs
/// without hand-copying a table the server would then be free to change.
pub const OPERATORS: &[OperatorSpec] = &[
  OperatorSpec::of(Op::Eq),
  OperatorSpec::of(Op::Ne),
  OperatorSpec::of(Op::Lt),
  OperatorSpec::of(Op::Lte),
  OperatorSpec::of(Op::Gt),
  OperatorSpec::of(Op::Gte),
  OperatorSpec::of(Op::Between),
  OperatorSpec::of(Op::Contains),
  OperatorSpec::of(Op::StartsWith),
  OperatorSpec::of(Op::EndsWith),
  OperatorSpec::of(Op::In),
  OperatorSpec::of(Op::Nin),
  OperatorSpec::of(Op::Matches),
  OperatorSpec::of(Op::IsNull),
  OperatorSpec::of(Op::IsNotNull),
];

impl Op {
  pub const TEXT: &'static [Op] = &[
    Op::Eq,
    Op::Ne,
    Op::Contains,
    Op::StartsWith,
    Op::EndsWith,
    Op::In,
    Op::IsNull,
    Op::IsNotNull,
  ];
  pub const NUMBER: &'static [Op] = &[
    Op::Eq,
    Op::Ne,
    Op::Lt,
    Op::Lte,
    Op::Gt,
    Op::Gte,
    Op::Between,
    Op::In,
    Op::IsNull,
    Op::IsNotNull,
  ];
  pub const BOOL: &'static [Op] = &[Op::Eq, Op::IsNull, Op::IsNotNull];
  pub const ENUM: &'static [Op] = &[Op::Eq, Op::Ne, Op::In, Op::Nin, Op::IsNull, Op::IsNotNull];
  pub const DATE: &'static [Op] =
    &[Op::Eq, Op::Lt, Op::Lte, Op::Gt, Op::Gte, Op::Between, Op::IsNull, Op::IsNotNull];
  /// A key pointing at another resource — an integer or a uuid carrying a `RelationHint`.
  pub const ID: &'static [Op] = &[Op::Eq, Op::Ne, Op::In, Op::Nin, Op::IsNull, Op::IsNotNull];
  pub const FULL_TEXT: &'static [Op] = &[Op::Matches];

  pub const fn arity(self) -> Arity {
    match self {
      Op::IsNull | Op::IsNotNull => Arity::Nullary,
      Op::In | Op::Nin => Arity::Many,
      Op::Between => Arity::Range,
      _ => Arity::One,
    }
  }

  /// The three that mean "somewhere inside the text", and therefore mean `ILIKE`.
  pub const fn is_substring(self) -> bool {
    matches!(self, Op::Contains | Op::StartsWith | Op::EndsWith)
  }

  pub const fn as_str(self) -> &'static str {
    match self {
      Op::Eq => "eq",
      Op::Ne => "ne",
      Op::Lt => "lt",
      Op::Lte => "lte",
      Op::Gt => "gt",
      Op::Gte => "gte",
      Op::Between => "between",
      Op::Contains => "contains",
      Op::StartsWith => "startsWith",
      Op::EndsWith => "endsWith",
      Op::In => "in",
      Op::Nin => "nin",
      Op::Matches => "matches",
      Op::IsNull => "isNull",
      Op::IsNotNull => "isNotNull",
    }
  }
}

impl std::fmt::Display for Op {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.as_str())
  }
}

/// What a key points at, for a client that wants to offer a picker rather than a raw id box.
///
/// It carries a path and a field name and nothing else — no label text and no language. What the
/// picker reads out of the related resource is the client's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationHint {
  /// The related resource's route, as a client would call it.
  pub resource:    &'static str,
  /// The field on that resource a client would show in place of the id.
  pub label_field: &'static str,
}

fn is_empty_slice(variants: &&'static [&'static str]) -> bool {
  variants.is_empty()
}

/// One declared field: what it is, how it is matched, what may be asked of it, and whether it can be
/// ordered by.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpec {
  /// The client-facing key. This is the query-string name, so it is also what a reader types.
  pub key:      &'static str,
  pub kind:     FieldKind,
  pub strategy: MatchStrategy,
  /// The operators this field accepts. Empty means the field is not filterable at all and exists
  /// only to be ordered by.
  pub ops:      &'static [Op],
  /// The accepted values, for `Enum`. Required there and forbidden everywhere else.
  #[serde(skip_serializing_if = "is_empty_slice")]
  pub variants: &'static [&'static str],
  pub sortable: bool,
  /// The key to resolve when ordering by this field, when that is not the field's own key. A field
  /// matched over several columns still has to order by exactly one.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub sort_key: Option<&'static str>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub relation: Option<RelationHint>,
}

impl FieldSpec {
  pub const fn new(
    key: &'static str,
    kind: FieldKind,
    strategy: MatchStrategy,
    ops: &'static [Op],
  ) -> Self {
    Self {
      key,
      kind,
      strategy,
      ops,
      variants: &[],
      sortable: false,
      sort_key: None,
      relation: None,
    }
  }

  /// A field that is only ever ordered by. It takes no operator, so it is not offered as a filter.
  pub const fn sort_only(key: &'static str, kind: FieldKind) -> Self {
    Self::new(key, kind, MatchStrategy::Comparison, &[]).with_sort()
  }

  pub const fn with_variants(mut self, variants: &'static [&'static str]) -> Self {
    self.variants = variants;
    self
  }

  pub const fn with_sort(mut self) -> Self {
    self.sortable = true;
    self
  }

  /// Sortable, but ordered by another declared key.
  pub const fn with_sort_key(mut self, sort_key: &'static str) -> Self {
    self.sortable = true;
    self.sort_key = Some(sort_key);
    self
  }

  pub const fn with_relation(mut self, resource: &'static str, label_field: &'static str) -> Self {
    self.relation = Some(RelationHint {
      resource,
      label_field,
    });
    self
  }

  /// Everything the field's *kind* could ever mean, before the strategy narrows it.
  pub const fn vocabulary(&self) -> &'static [Op] {
    if matches!(self.strategy, MatchStrategy::FullText) {
      return Op::FULL_TEXT;
    }

    if self.relation.is_some() {
      return Op::ID;
    }

    match self.kind {
      FieldKind::Text => Op::TEXT,
      FieldKind::Integer | FieldKind::Decimal => Op::NUMBER,
      FieldKind::Boolean => Op::BOOL,
      FieldKind::Enum => Op::ENUM,
      FieldKind::DateTime => Op::DATE,
      FieldKind::Uuid => Op::ID,
    }
  }

  /// Whether the kind *and* the strategy can both mean this operator.
  ///
  /// The strategy is what keeps `contains` off a `FullText` field: the two lower to different SQL,
  /// and a field declared for one must never be served by the other.
  pub fn allows(&self, op: Op) -> bool {
    match self.strategy {
      MatchStrategy::FullText => op == Op::Matches,
      MatchStrategy::Custom => self.vocabulary().contains(&op) || op == Op::Matches,
      MatchStrategy::Substring => self.vocabulary().contains(&op),
      MatchStrategy::Comparison => {
        self.vocabulary().contains(&op) && !op.is_substring() && op != Op::Matches
      }
    }
  }

  pub fn validate(&self) -> Result<(), FilterSchemaError> {
    if self.key.is_empty() {
      return Err(FilterSchemaError::EmptyKey);
    }

    if RESERVED_KEYS.contains(&self.key) {
      return Err(FilterSchemaError::ReservedKey(self.key));
    }

    if !is_valid_key(self.key) {
      return Err(FilterSchemaError::InvalidKey(self.key));
    }

    if matches!(self.strategy, MatchStrategy::Substring | MatchStrategy::FullText)
      && !matches!(self.kind, FieldKind::Text)
    {
      return Err(FilterSchemaError::StrategyKindMismatch {
        key:      self.key,
        strategy: self.strategy,
        kind:     self.kind,
      });
    }

    match (self.kind, self.variants.is_empty()) {
      (FieldKind::Enum, true) => return Err(FilterSchemaError::MissingVariants(self.key)),
      (FieldKind::Enum, false) => {}
      (_, false) => return Err(FilterSchemaError::UnexpectedVariants(self.key)),
      (_, true) => {}
    }

    for op in self.ops {
      if !self.allows(*op) {
        return Err(FilterSchemaError::OpNotAllowed {
          key: self.key,
          op:  *op,
        });
      }
    }

    if self.ops.is_empty() && !self.sortable {
      return Err(FilterSchemaError::InertField(self.key));
    }

    if self.sort_key.is_some() && !self.sortable {
      return Err(FilterSchemaError::SortKeyWithoutSort(self.key));
    }

    Ok(())
  }
}

/// A key travels in a query string and is read back by a person, so it stays to the identifier
/// shape both a URL and a code generator can carry unescaped.
fn is_valid_key(key: &str) -> bool {
  let mut chars = key.chars();

  match chars.next() {
    Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
    _ => return false,
  }

  chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A declaration that cannot be served. Every variant is a wiring mistake, caught where the
/// resource's route is mounted rather than on the request that would have tripped over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterSchemaError {
  EmptyKey,
  ReservedKey(&'static str),
  InvalidKey(&'static str),
  DuplicateKey(&'static str),
  MissingVariants(&'static str),
  UnexpectedVariants(&'static str),
  InertField(&'static str),
  SortKeyWithoutSort(&'static str),
  OpNotAllowed {
    key: &'static str,
    op:  Op,
  },
  StrategyKindMismatch {
    key:      &'static str,
    strategy: MatchStrategy,
    kind:     FieldKind,
  },
  /// The default sort names a key that is not declared, or one that is declared but not sortable.
  /// A list with no usable default sort paginates over an undefined order.
  UnsortableDefault(&'static str),
}

impl std::fmt::Display for FilterSchemaError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::EmptyKey => write!(f, "a field key may not be empty"),
      Self::ReservedKey(key) => write!(f, "`{key}` is reserved by the list envelope"),
      Self::InvalidKey(key) => write!(f, "`{key}` is not a usable field key"),
      Self::DuplicateKey(key) => write!(f, "`{key}` is declared twice"),
      Self::MissingVariants(key) => write!(f, "`{key}` is an enum and declares no variants"),
      Self::UnexpectedVariants(key) => write!(f, "`{key}` declares variants but is not an enum"),
      Self::InertField(key) => write!(f, "`{key}` takes no operator and is not sortable"),
      Self::SortKeyWithoutSort(key) => write!(f, "`{key}` names a sort key but is not sortable"),
      Self::OpNotAllowed { key, op } => write!(f, "`{key}` may not take `{op}`"),
      Self::StrategyKindMismatch {
        key,
        strategy,
        kind,
      } => write!(f, "`{key}` cannot match {kind:?} with {strategy:?}"),
      Self::UnsortableDefault(key) => write!(f, "the default sort `{key}` is not a sortable field"),
    }
  }
}

impl std::error::Error for FilterSchemaError {}

#[cfg(test)]
mod tests {
  use super::*;

  fn text(key: &'static str) -> FieldSpec {
    FieldSpec::new(key, FieldKind::Text, MatchStrategy::Substring, &[Op::Contains])
  }

  #[test]
  fn reserved_keys_are_refused() {
    for &key in RESERVED_KEYS {
      let spec = text(key);
      assert_eq!(spec.validate(), Err(FilterSchemaError::ReservedKey(key)));
    }
  }

  #[test]
  fn full_text_refuses_comparison_operators() {
    let spec = FieldSpec::new("body", FieldKind::Text, MatchStrategy::FullText, &[Op::Matches]);
    assert!(spec.validate().is_ok());

    for op in [Op::Contains, Op::Eq, Op::StartsWith, Op::In] {
      assert!(!spec.allows(op), "full text must not accept `{op}`");
    }
  }

  #[test]
  fn comparison_refuses_substring_operators() {
    let spec = FieldSpec::new("name", FieldKind::Text, MatchStrategy::Comparison, &[Op::Contains]);

    assert_eq!(
      spec.validate(),
      Err(FilterSchemaError::OpNotAllowed {
        key: "name",
        op:  Op::Contains,
      })
    );
  }

  #[test]
  fn substring_and_full_text_need_text() {
    let spec = FieldSpec::new("size", FieldKind::Integer, MatchStrategy::Substring, &[Op::Contains]);
    assert!(matches!(spec.validate(), Err(FilterSchemaError::StrategyKindMismatch { .. })));
  }

  #[test]
  fn a_relation_hint_narrows_an_integer_to_the_identity_vocabulary() {
    let plain = FieldSpec::new("amount", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Gte]);
    assert!(plain.validate().is_ok());

    let related = FieldSpec::new("board_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Gte])
      .with_relation("/boards", "name");

    assert!(matches!(related.validate(), Err(FilterSchemaError::OpNotAllowed { .. })));
    assert!(related.allows(Op::Nin));
    assert!(!plain.allows(Op::Nin));
  }

  #[test]
  fn an_enum_must_declare_its_variants() {
    let spec = FieldSpec::new("status", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq]);
    assert_eq!(spec.validate(), Err(FilterSchemaError::MissingVariants("status")));

    let spec = spec.with_variants(&["draft", "published"]);
    assert!(spec.validate().is_ok());
  }

  #[test]
  fn variants_belong_to_enums_only() {
    let spec = text("title").with_variants(&["a"]);
    assert_eq!(spec.validate(), Err(FilterSchemaError::UnexpectedVariants("title")));
  }

  #[test]
  fn a_field_that_neither_filters_nor_sorts_is_refused() {
    let spec = FieldSpec::new("dead", FieldKind::Text, MatchStrategy::Comparison, &[]);
    assert_eq!(spec.validate(), Err(FilterSchemaError::InertField("dead")));

    assert!(FieldSpec::sort_only("created_at", FieldKind::DateTime).validate().is_ok());
  }

  #[test]
  fn key_shape_is_enforced() {
    assert_eq!(text("").validate(), Err(FilterSchemaError::EmptyKey));
    assert_eq!(text("board.id").validate(), Err(FilterSchemaError::InvalidKey("board.id")));
    assert_eq!(text("1st").validate(), Err(FilterSchemaError::InvalidKey("1st")));
    assert!(text("board_id").validate().is_ok());
  }

  #[test]
  fn the_served_vocabulary_holds_every_operator_a_kind_can_mean() {
    let served: Vec<Op> = OPERATORS.iter().map(|entry| entry.op).collect();

    for family in [Op::TEXT, Op::NUMBER, Op::BOOL, Op::ENUM, Op::DATE, Op::ID, Op::FULL_TEXT] {
      for op in family {
        assert!(served.contains(op), "`{op}` is declarable but not served");
      }
    }

    for entry in OPERATORS {
      assert_eq!(entry.arity, entry.op.arity());
    }
  }

  #[test]
  fn arity_says_how_many_operands_an_operator_takes() {
    assert_eq!(Op::IsNull.arity(), Arity::Nullary);
    assert_eq!(Op::Eq.arity(), Arity::One);
    assert_eq!(Op::In.arity(), Arity::Many);
    assert_eq!(Op::Between.arity(), Arity::Range);
  }
}
