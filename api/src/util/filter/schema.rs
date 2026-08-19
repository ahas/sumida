//! The trait a resource implements to say what its list accepts.
//!
//! One implementation sits next to the resource's own `Filter` struct, which is the only place that
//! knows both the client-facing key and the column behind it. Nothing central holds a table keyed by
//! resource name: a resource that is added brings its own declaration, and a resource that is
//! removed takes its declaration with it.

use sea_orm::sea_query::SimpleExpr;

use super::spec::{FieldSpec, FilterSchemaError};

pub trait FilterSchema {
  /// Every field this resource opens to filtering, ordering, or both.
  fn fields() -> &'static [FieldSpec];

  /// The column expression behind one key, over this resource's own entity or over a table its list
  /// query has already joined.
  ///
  /// A field whose match does not reduce to a single column — a `Custom` strategy spanning several
  /// columns, say — returns `None`; its expression is the resource's own to build.
  ///
  /// Callers do not reach this directly. `resolve` does, and only after the key has been found in
  /// the declaration, so an undeclared key can never arrive here.
  fn column(key: &str) -> Option<SimpleExpr>;

  /// The key a list orders by when the request names none, or names one this resource does not
  /// declare as sortable. Direction stays the request's `order`, ascending unless asked otherwise.
  fn default_sort() -> &'static str;

  fn field(key: &str) -> Option<&'static FieldSpec> {
    Self::fields().iter().find(|field| field.key == key)
  }

  /// A key to an expression, refusing anything the resource did not declare.
  ///
  /// This is the whole of the guarantee that a filter cannot name a column: an undeclared key has no
  /// expression, so there is nothing to put in the query.
  fn resolve(key: &str) -> Option<SimpleExpr> {
    Self::field(key)?;
    Self::column(key)
  }

  /// The expression to order by for a declared, sortable key — its own, or the distinct sort key it
  /// names.
  fn sort_column(key: &str) -> Option<SimpleExpr> {
    let field = Self::field(key)?;

    if !field.sortable {
      return None;
    }

    Self::column(field.sort_key.unwrap_or(field.key))
  }

  /// Checks the declaration as a whole. Run where the resource's routes are mounted, so a
  /// declaration that cannot be served stops the process instead of a request.
  fn validate() -> Result<(), FilterSchemaError> {
    let fields = Self::fields();

    for (index, field) in fields.iter().enumerate() {
      field.validate()?;

      if fields[..index].iter().any(|earlier| earlier.key == field.key) {
        return Err(FilterSchemaError::DuplicateKey(field.key));
      }
    }

    match Self::field(Self::default_sort()) {
      Some(field) if field.sortable => Ok(()),
      _ => Err(FilterSchemaError::UnsortableDefault(Self::default_sort())),
    }
  }
}

#[cfg(test)]
mod tests {
  use sea_orm::sea_query::Expr;

  use super::super::spec::{FieldKind, FieldSpec, MatchStrategy, Op};
  use super::*;

  struct Sample;

  impl FilterSchema for Sample {
    fn fields() -> &'static [FieldSpec] {
      const FIELDS: &[FieldSpec] = &[
        FieldSpec::new("title", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains])
          .with_sort_key("sort_title"),
        FieldSpec::new("body", FieldKind::Text, MatchStrategy::FullText, &[Op::Matches]),
        FieldSpec::sort_only("created_at", FieldKind::DateTime),
      ];

      FIELDS
    }

    fn column(key: &str) -> Option<SimpleExpr> {
      match key {
        "title" | "sort_title" | "created_at" => {
          Some(Expr::col(sea_orm::sea_query::Alias::new(key)).into())
        }
        _ => None,
      }
    }

    fn default_sort() -> &'static str {
      "created_at"
    }
  }

  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    assert!(Sample::resolve("title").is_some());
    assert!(Sample::resolve("password").is_none());
  }

  #[test]
  fn sorting_needs_a_sortable_field_and_follows_its_sort_key() {
    assert!(Sample::sort_column("body").is_none());
    assert!(Sample::sort_column("created_at").is_some());
    // `title` is matched over one column and ordered by another; resolution follows the sort key.
    assert!(Sample::sort_column("title").is_some());
    assert!(Sample::resolve("sort_title").is_none());
  }

  #[test]
  fn a_sound_declaration_validates() {
    assert!(Sample::validate().is_ok());
  }

  struct Undefaulted;

  impl FilterSchema for Undefaulted {
    fn fields() -> &'static [FieldSpec] {
      const FIELDS: &[FieldSpec] =
        &[FieldSpec::new("body", FieldKind::Text, MatchStrategy::FullText, &[Op::Matches])];

      FIELDS
    }

    fn column(_key: &str) -> Option<SimpleExpr> {
      None
    }

    fn default_sort() -> &'static str {
      "body"
    }
  }

  #[test]
  fn a_default_sort_must_name_a_sortable_field() {
    assert_eq!(Undefaulted::validate(), Err(FilterSchemaError::UnsortableDefault("body")));
  }

  struct Duplicated;

  impl FilterSchema for Duplicated {
    fn fields() -> &'static [FieldSpec] {
      const FIELDS: &[FieldSpec] = &[
        FieldSpec::sort_only("created_at", FieldKind::DateTime),
        FieldSpec::sort_only("created_at", FieldKind::DateTime),
      ];

      FIELDS
    }

    fn column(_key: &str) -> Option<SimpleExpr> {
      None
    }

    fn default_sort() -> &'static str {
      "created_at"
    }
  }

  #[test]
  fn a_key_may_not_be_declared_twice() {
    assert_eq!(Duplicated::validate(), Err(FilterSchemaError::DuplicateKey("created_at")));
  }
}
