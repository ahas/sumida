//! The definition registry — the compiled rule set (design §5) and the namespace inventory.
//!
//! The schema text is `include_str!`-ed, so it is baked into the binary and a rule edit is a
//! deploy, not a runtime change (E1). It is parsed once behind a `OnceLock` and validated
//! immediately; a schema that does not parse or does not validate produces an error every entry
//! point returns, and `Authz::boot` turns that into a refusal to start.
//!
//! All four products' rule sets live here rather than in each consumer, because all four are path
//! dependencies on this one working tree and a namespace with no rule is a live resource that is
//! unreachable or unprotected. Splitting them per file keeps ownership legible; a later phase moves
//! each consumer's file into the consumer and registers it, at which point validation rule 8 gets
//! its inventory from the consumer instead of from here.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::ir::*;
use super::{AuthzError, parse};

/// One product's schema file.
pub struct Product {
  pub name:   &'static str,
  pub file:   &'static str,
  pub text:   &'static str,
  /// A table that exists if and only if this product is deployed in the database being inspected.
  ///
  /// Needed because sumida is one working tree shared by four products and no database has all
  /// four's tables — but two of them own a table of the *same name* (`jobs` is trainer's and site-app's,
  /// with different columns), so "the table exists" cannot decide whose rules to check.
  pub marker: &'static str,
}

/// Every schema file compiled into this binary.
///
/// The namespace inventory in these files is the enumeration produced by grepping `service_ap!`
/// and `ap::ns(` across sumida, trainer, site-app and lab-app (`origin/bs`). Validation rule 8 refuses to
/// boot if any entry has no definition behind it.
pub const PRODUCTS: &[Product] = &[
  Product {
    name:   "sumida",
    file:   "schema.zed",
    text:   include_str!("schema.zed"),
    marker: "users",
  },
  Product {
    name:   "trainer",
    file:   "schema.trainer.zed",
    text:   include_str!("schema.trainer.zed"),
    marker: "tasks",
  },
  Product {
    name:   "site-app",
    file:   "schema.site-app.zed",
    text:   include_str!("schema.site-app.zed"),
    marker: "projects",
  },
  Product {
    name:   "lab-app",
    file:   "schema.lab-app.zed",
    text:   include_str!("schema.lab-app.zed"),
    marker: "sites",
  },
];

#[derive(Debug)]
pub struct Registry {
  definitions: Vec<Definition>,
  by_name:     HashMap<String, usize>,
  namespaces:  Vec<NamespaceEntry>,
  /// Which product's schema file declared each definition. Report-only, and used to say *whose*
  /// rule is unverifiable when its table is absent from this database.
  owner:       HashMap<String, &'static str>,
}

impl Registry {
  /// Parses and validates every schema file. Called once; the result is memoised by `global`.
  pub fn load() -> Result<Self, AuthzError> {
    Self::from_sources(PRODUCTS)
  }

  /// Parses and validates an arbitrary set of schema files.
  ///
  /// Public so the validation tests can hand it a schema with one deliberate defect and assert that
  /// the process would refuse to start. A boot check nobody has watched fail is a boot check nobody
  /// knows works.
  pub fn from_sources(products: &[Product]) -> Result<Self, AuthzError> {
    let mut registry = Registry {
      definitions: Vec::new(),
      by_name:     HashMap::new(),
      namespaces:  Vec::new(),
      owner:       HashMap::new(),
    };

    for product in products {
      let file = parse::parse(product.file, product.name, product.text).map_err(AuthzError::Parse)?;
      for definition in file.definitions {
        if registry.by_name.contains_key(&definition.name) {
          return Err(AuthzError::Schema(format!(
            "definition `{}` is declared twice; the second is in {}",
            definition.name, product.file
          )));
        }
        registry.owner.insert(definition.name.clone(), product.name);
        registry.by_name.insert(definition.name.clone(), registry.definitions.len());
        registry.definitions.push(definition);
      }
      registry.namespaces.extend(file.namespaces);
    }

    super::validate::validate(&registry)?;
    Ok(registry)
  }

  /// The process-wide registry. The error is reproduced on every call, so a boot that ignores it
  /// still cannot get a usable evaluator.
  pub fn global() -> Result<&'static Registry, AuthzError> {
    static REGISTRY: OnceLock<Result<Registry, AuthzError>> = OnceLock::new();
    match REGISTRY.get_or_init(Registry::load) {
      Ok(registry) => Ok(registry),
      Err(error) => Err(error.clone()),
    }
  }

  pub fn definitions(&self) -> &[Definition] {
    &self.definitions
  }

  pub fn namespaces(&self) -> &[NamespaceEntry] {
    &self.namespaces
  }

  pub fn get(&self, name: &str) -> Option<&Definition> {
    self.by_name.get(name).map(|i| &self.definitions[*i])
  }

  pub fn require(&self, name: &str) -> Result<&Definition, AuthzError> {
    self.get(name).ok_or_else(|| AuthzError::UnknownDefinition(name.to_owned()))
  }

  pub fn owner_of(&self, definition: &str) -> Option<&'static str> {
    self.owner.get(definition).copied()
  }

  /// Every `<table>.<column>` the rules read, with the definition that reads it. This is what
  /// `Authz::boot` checks against the live database: a table that is absent means the product is
  /// not deployed here, a column that is absent from a table that *is* present is a fatal schema
  /// drift.
  pub fn column_dependencies(&self) -> Vec<ColumnDependency> {
    let mut out = Vec::new();
    for definition in &self.definitions {
      if let Some(table) = &definition.table {
        out.push(ColumnDependency {
          definition: definition.name.clone(),
          table:      table.table.clone(),
          column:     table.id_col.clone(),
        });
      }
      for relation in &definition.relations {
        let RelationSource::Derived(derivation) = &relation.source else { continue };
        out.push(ColumnDependency {
          definition: definition.name.clone(),
          table:      derivation.table.clone(),
          column:     derivation.column.clone(),
        });
        if let Correlation::Of(column) = &derivation.correlation {
          out.push(ColumnDependency {
            definition: definition.name.clone(),
            table:      derivation.table.clone(),
            column:     column.clone(),
          });
        }
        for hop in &derivation.hops {
          out.push(ColumnDependency {
            definition: definition.name.clone(),
            table:      hop.table.clone(),
            column:     hop.key_col.clone(),
          });
          out.push(ColumnDependency {
            definition: definition.name.clone(),
            table:      hop.table.clone(),
            column:     hop.next_col.clone(),
          });
        }
      }
    }
    out.sort_by(|a, b| (&a.table, &a.column, &a.definition).cmp(&(&b.table, &b.column, &b.definition)));
    out.dedup_by(|a, b| a.table == b.table && a.column == b.column && a.definition == b.definition);
    out
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDependency {
  pub definition: String,
  pub table:      String,
  pub column:     String,
}
