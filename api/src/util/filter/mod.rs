//! The one place a resource says what its list can be filtered and ordered by.
//!
//! A resource declares a `&'static [FieldSpec]` next to its `Filter` struct and implements
//! [`FilterSchema`] over it. That single declaration is what the sort resolver reads and what the
//! descriptor endpoint serves, so the fields a client is offered and the fields the server will act
//! on cannot drift apart.
//!
//! **A filter only ever narrows an already authorized scope.** The list query is built from the read
//! scope first; the declared fields are applied inside it. Nothing here grants access to a row, and
//! nothing here can widen a scope — a key that is not declared has no expression at all, so a filter
//! cannot name a column the resource did not open.
//!
//! The same declaration also compiles a condition tree, which says how several comparisons combine
//! rather than only ANDing one per key. It is read from the reserved `where` parameter, checked
//! against the same declaration, and added to the query as one condition — so it narrows on exactly
//! the terms the flat parameters do.

use sea_orm_crud::OneOrMany;

pub mod condition;
pub mod descriptor;
pub mod schema;
pub mod spec;

pub use condition::{
  CONDITION_KEY,
  ConditionError,
  ConditionNode,
  ConditionQuery,
  ConditionTree,
  MAX_CONDITION_BYTES,
  MAX_CONDITION_DEPTH,
  MAX_CONDITION_FULL_TEXT,
  MAX_CONDITION_NODES,
  MAX_CONDITION_SUBSTRING,
  MAX_CONDITION_TEXT_LENGTH,
  MAX_CONDITION_VALUES,
};
pub use descriptor::{
  DESCRIPTOR_PATH,
  FilterDescriptor,
  FilterLimits,
  filter_descriptor_route,
  filter_descriptor_router,
};
pub use schema::FilterSchema;
pub use spec::{
  Arity,
  FieldKind,
  FieldSpec,
  FilterSchemaError,
  MatchStrategy,
  OPERATORS,
  Op,
  OperatorSpec,
  RESERVED_KEYS,
  RelationHint,
};

/// A multi-value parameter that actually names something.
///
/// An empty value (`?id=`) is an empty list, and an empty list means unfiltered — a form that
/// submits its untouched boxes must not narrow the page to nothing. Reading a parameter through
/// this is what keeps that spelling from reaching the query as an `IN ()`.
pub fn listed<T>(values: Option<&OneOrMany<T>>) -> Option<&OneOrMany<T>> {
  values.filter(|v| !v.is_empty())
}
