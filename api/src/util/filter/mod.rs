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

pub mod descriptor;
pub mod schema;
pub mod spec;

pub use descriptor::{DESCRIPTOR_PATH, FilterDescriptor, FilterLimits, filter_descriptor_router};
pub use schema::FilterSchema;
pub use spec::{
  Arity,
  FieldKind,
  FieldSpec,
  FilterSchemaError,
  MatchStrategy,
  Op,
  RESERVED_KEYS,
  RelationHint,
};
