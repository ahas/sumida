//! Relational Zanzibar — the evaluator.
//!
//! **Nothing calls any of this yet.** `util/ap.rs` and `access_policies` remain the only authority
//! for every decision the server makes; this module is dead code that compiles, validates and can
//! be tested. That isolation is deliberate — it is what makes the phase reviewable and revertible.
//!
//! ```text
//! schema*.zed  ──parse──▶  ir  ──validate──▶  registry  ──compile──▶  one SQL predicate
//!                                   │                                        │
//!                                   └── aborts boot, naming the offender      └── drops into an
//!                                                                                existing WHERE
//! ```
//!
//! * `subject`  — one identity expansion, replacing four copies and a hand-parser.
//! * `ir`       — the rewrite representation.
//! * `parse`    — the SpiceDB-flavoured schema language of §4.1.
//! * `registry` — the rule set, memoised, with the namespace inventory.
//! * `validate` — the eight boot rules. Failure aborts start-up.
//! * `compile`  — a rewrite becomes one boolean `SimpleExpr`.

pub mod compile;
pub mod ir;
pub mod parse;
pub mod registry;
pub mod subject;
pub mod validate;

#[cfg(test)]
mod golden_sql_tests;

use std::collections::HashSet;

use error::SumidaError;
use sea_orm::sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr};
use sea_orm::{ConnectionTrait, EntityTrait, FromQueryResult, QueryFilter, Select, Statement};

use self::compile::Compiler;
use self::ir::RelationSource;
use self::parse::ParseError;
use self::registry::Registry;
use self::subject::SubjectSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthzError {
  Parse(ParseError),
  /// Every structural failure at once, so a schema is fixed in one pass rather than one boot at a
  /// time.
  Validation(Vec<String>),
  Schema(String),
  UnknownDefinition(String),
  UnknownRelation {
    definition: String,
    relation:   String,
  },
  UnknownPermission {
    definition: String,
    permission: String,
  },
  /// A rule that parses but cannot become one SQL statement. §6.5 lists the shapes; the design's
  /// requirement is that each is **named and refused**, never degraded into a second query.
  NotCompilable(String),
  Database(String),
}

impl std::fmt::Display for AuthzError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      AuthzError::Parse(error) => write!(f, "authz schema: {error}"),
      AuthzError::Validation(errors) => {
        writeln!(f, "authz schema is invalid ({} problem(s)):", errors.len())?;
        for error in errors {
          writeln!(f, "  · {error}")?;
        }
        Ok(())
      }
      AuthzError::Schema(message) => write!(f, "authz schema: {message}"),
      AuthzError::UnknownDefinition(name) => write!(f, "authz: no definition `{name}`"),
      AuthzError::UnknownRelation {
        definition,
        relation,
      } => {
        write!(f, "authz: `{definition}` has no relation `{relation}`")
      }
      AuthzError::UnknownPermission {
        definition,
        permission,
      } => {
        write!(f, "authz: `{definition}` has no permission `{permission}`")
      }
      AuthzError::NotCompilable(message) => write!(f, "authz: {message}"),
      AuthzError::Database(message) => write!(f, "authz: {message}"),
    }
  }
}

impl std::error::Error for AuthzError {}

impl From<AuthzError> for SumidaError {
  fn from(error: AuthzError) -> Self {
    SumidaError::Unknown(500, error.to_string())
  }
}

impl From<sea_orm::DbErr> for AuthzError {
  fn from(error: sea_orm::DbErr) -> Self {
    AuthzError::Database(error.to_string())
  }
}

/// One object's id.
///
/// Two variants rather than a string, because the id reaches two columns of different types —
/// `relation_tuples.resource_id` is text, `org_closure.descendant_id` is integer — and collapsing
/// them into text would mean a cast on the join that the closure index cannot serve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectKey {
  Int(i32),
  Text(String),
}

impl ObjectKey {
  pub(crate) fn as_expr(&self) -> SimpleExpr {
    match self {
      ObjectKey::Int(value) => Expr::val(*value).into(),
      ObjectKey::Text(value) => Expr::val(value.clone()).into(),
    }
  }
}

impl From<i32> for ObjectKey {
  fn from(value: i32) -> Self {
    ObjectKey::Int(value)
  }
}

impl From<&str> for ObjectKey {
  fn from(value: &str) -> Self {
    ObjectKey::Text(value.to_owned())
  }
}

impl From<String> for ObjectKey {
  fn from(value: String) -> Self {
    ObjectKey::Text(value)
  }
}

impl From<uuid::Uuid> for ObjectKey {
  fn from(value: uuid::Uuid) -> Self {
    ObjectKey::Text(value.to_string())
  }
}

/// The entry points. Deliberately small: everything else is a rule.
pub struct Authz;

impl Authz {
  /// The validated rule set. The first call parses and validates; every later call is a lookup.
  pub fn registry() -> Result<&'static Registry, AuthzError> {
    Registry::global()
  }

  /// Adds the compiled rule as a `WHERE` predicate. Nothing else on the query is touched — not the
  /// projection, not the ordering, not the limit — which is what keeps filter, `COUNT`, `ORDER BY`
  /// and `LIMIT/OFFSET` in one statement.
  pub fn filter<E: EntityTrait>(
    definition: &str,
    permission: &str,
    subject: &SubjectSet,
    query: Select<E>,
  ) -> Result<Select<E>, AuthzError> {
    let registry = Self::registry()?;
    let predicate = Compiler::new(registry, subject).filter_predicate(definition, permission)?;
    Ok(query.filter(predicate))
  }

  /// A point check on one object.
  pub async fn check<C: ConnectionTrait, K: Into<ObjectKey>>(
    definition: &str,
    id: K,
    permission: &str,
    subject: &SubjectSet,
    db: &C,
  ) -> Result<bool, AuthzError> {
    #[derive(FromQueryResult)]
    struct Answer {
      allowed: bool,
    }

    let registry = Self::registry()?;
    let statement =
      Compiler::new(registry, subject).object_check_statement(definition, permission, &id.into())?;
    let row = Answer::find_by_statement(db.get_database_backend().build(&statement)).one(db).await?;

    Ok(row.map(|r| r.allowed).unwrap_or(false))
  }

  pub async fn try_check<C: ConnectionTrait, K: Into<ObjectKey>>(
    definition: &str,
    id: K,
    permission: &str,
    subject: &SubjectSet,
    db: &C,
  ) -> Result<(), SumidaError> {
    if Self::check(definition, id, permission, subject, db).await? {
      Ok(())
    } else {
      Err(SumidaError::NoAccessPermission)
    }
  }

  /// The predicate as Postgres SQL text, values inlined.
  ///
  /// This is what the golden tests assert against. A compiled rule is the only place the whole
  /// authorization decision is visible at once, so being able to read it — in a test, in a review,
  /// in an incident — is worth an entry point of its own.
  pub fn predicate_sql(
    definition: &str,
    permission: &str,
    subject: &SubjectSet,
  ) -> Result<String, AuthzError> {
    let registry = Self::registry()?;
    let predicate = Compiler::new(registry, subject).filter_predicate(definition, permission)?;
    Ok(render(predicate))
  }

  /// The point-check predicate as Postgres SQL text, values inlined.
  pub fn object_predicate_sql<K: Into<ObjectKey>>(
    definition: &str,
    id: K,
    permission: &str,
    subject: &SubjectSet,
  ) -> Result<String, AuthzError> {
    let registry = Self::registry()?;
    let predicate =
      Compiler::new(registry, subject).object_predicate(definition, permission, &id.into())?;
    Ok(render(predicate))
  }

  /// Validates the rule set and reports which definitions this database can actually back.
  ///
  /// Three outcomes per definition, and the difference between the last two is the whole point:
  ///
  /// * **Active** — every table and column the rule reads is present.
  /// * **Absent** — the product's marker table is not in this database, so the product is not
  ///   deployed here. Not an error: sumida's schema is shared by four products and no database has
  ///   all four.
  /// * **Drift** — the product *is* deployed and a column the rule reads is missing. That is a rule
  ///   and a schema disagreeing, which is fatal for anything that would evaluate the rule;
  ///   `boot_or_abort` treats it as such.
  pub async fn boot<C: ConnectionTrait>(db: &C) -> Result<BootReport, AuthzError> {
    let registry = Self::registry()?; // parse + validate; aborts here on any structural failure
    let present = live_columns(db).await?;

    let deployed: HashSet<&str> = registry::PRODUCTS
      .iter()
      .filter(|product| present.iter().any(|(table, _)| table == product.marker))
      .map(|product| product.name)
      .collect();

    let dependencies = registry.column_dependencies();
    let entries = registry
      .definitions()
      .iter()
      .map(|definition| {
        let product = registry.owner_of(&definition.name).unwrap_or("sumida");
        let mine: Vec<&registry::ColumnDependency> = dependencies
          .iter()
          .filter(|dependency| dependency.definition == definition.name)
          .collect();
        DefinitionStatus {
          definition: definition.name.clone(),
          product,
          state: classify(product, &mine, deployed.contains(product), &present),
        }
      })
      .collect();

    Ok(BootReport { entries })
  }

  /// `boot`, with drift treated as fatal. This is what a consumer calls at start-up once it
  /// registers.
  pub async fn boot_or_abort<C: ConnectionTrait>(db: &C) -> Result<BootReport, AuthzError> {
    let report = Self::boot(db).await?;
    let drifted: Vec<String> = report
      .entries
      .iter()
      .filter_map(|entry| match &entry.state {
        DefinitionState::Drift { missing_columns } => Some(format!(
          "{} reads {}, which this database does not have",
          entry.definition,
          missing_columns.join(", ")
        )),
        _ => None,
      })
      .collect();

    if drifted.is_empty() { Ok(report) } else { Err(AuthzError::Validation(drifted)) }
  }

  /// Rewrites the `relation_definitions` / `permission_definitions` projection (E6) from the
  /// compiled registry, in one transaction's worth of statements.
  ///
  /// The projection is never read by the evaluator. It exists for the foreign key — three products
  /// write policy rows by raw SQL and by `ActiveModel` insert, and the FK is the only thing that
  /// can stop them writing a relation nobody declared — and for `GET /authz/schema`.
  ///
  /// A relation that has disappeared from the rules but still has tuples pointing at it is **not**
  /// deleted: that is a schema change with live grants behind it, and silently dropping either the
  /// vocabulary or the grants is a permission change nobody asked for. It is reported instead.
  pub async fn sync_schema<C: ConnectionTrait>(db: &C) -> Result<(), AuthzError> {
    let registry = Self::registry()?;
    let mut wanted: Vec<(String, String)> = Vec::new();

    for definition in registry.definitions() {
      for relation in &definition.relations {
        wanted.push((definition.name.clone(), relation.name.clone()));
        let (kind, subjects, wildcard, source) = match &relation.source {
          RelationSource::Stored {
            subjects,
            allows_wildcard,
          } => (
            "stored",
            subjects.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            *allows_wildcard,
            None,
          ),
          RelationSource::Derived(derivation) => (
            "derived",
            Vec::new(),
            false,
            Some(serde_json::to_string(derivation).unwrap_or_else(|_| "null".to_owned())),
          ),
        };

        db.execute(Statement::from_sql_and_values(
          db.get_database_backend(),
          r#"
          INSERT INTO relation_definitions
            (resource_type, relation, kind, subject_types, allows_wildcard, source)
          VALUES ($1, $2, $3, $4, $5, $6::jsonb)
          ON CONFLICT (resource_type, relation) DO UPDATE SET
            kind = EXCLUDED.kind, subject_types = EXCLUDED.subject_types,
            allows_wildcard = EXCLUDED.allows_wildcard, source = EXCLUDED.source
          "#,
          [
            definition.name.clone().into(),
            relation.name.clone().into(),
            kind.into(),
            subjects.into(),
            wildcard.into(),
            source.into(),
          ],
        ))
        .await?;
      }

      for (permission, rewrite) in &definition.permissions {
        db.execute(Statement::from_sql_and_values(
          db.get_database_backend(),
          r#"
          INSERT INTO permission_definitions (resource_type, permission, rewrite)
          VALUES ($1, $2, $3::jsonb)
          ON CONFLICT (resource_type, permission) DO UPDATE SET rewrite = EXCLUDED.rewrite
          "#,
          [
            definition.name.clone().into(),
            permission.clone().into(),
            serde_json::to_string(rewrite).unwrap_or_else(|_| "null".to_owned()).into(),
          ],
        ))
        .await?;
      }
    }

    // Anything the rules no longer declare, and that nothing points at, goes.
    let keep = wanted
      .iter()
      .map(|(definition, relation)| format!("('{definition}','{relation}')"))
      .collect::<Vec<_>>()
      .join(",");
    db.execute_unprepared(&format!(
      "DELETE FROM relation_definitions d
        WHERE (d.resource_type, d.relation) NOT IN ({keep})
          AND NOT EXISTS (SELECT 1 FROM relation_tuples t
                          WHERE t.resource_type = d.resource_type AND t.relation = d.relation)"
    ))
    .await?;

    #[derive(FromQueryResult)]
    struct Stranded {
      resource_type: String,
      relation:      String,
      tuples:        i64,
    }

    let stranded = Stranded::find_by_statement(Statement::from_string(
      db.get_database_backend(),
      format!(
        "SELECT d.resource_type, d.relation, count(t.id) AS tuples
           FROM relation_definitions d
           JOIN relation_tuples t
             ON t.resource_type = d.resource_type AND t.relation = d.relation
          WHERE (d.resource_type, d.relation) NOT IN ({keep})
          GROUP BY 1, 2"
      ),
    ))
    .all(db)
    .await?;

    if !stranded.is_empty() {
      return Err(AuthzError::Validation(
        stranded
          .into_iter()
          .map(|row| {
            format!(
              "`{}#{}` is no longer in the rules but {} tuple(s) still grant it; migrate or revoke \
               them before removing the relation",
              row.resource_type, row.relation, row.tuples
            )
          })
          .collect(),
      ));
    }

    Ok(())
  }
}

/// Decides one definition's state from what the database actually holds.
///
/// A pure function so the three outcomes can be exercised without a database. The distinction it
/// draws is the whole value of the check: **Absent** is a deployment fact (sumida's board and
/// membership features are optional, and no database has all four products' tables), while
/// **Drift** is a rule and a schema disagreeing — which fails closed only by accident, as a 500 on
/// the first request that evaluates the rule.
fn classify(
  product: &str,
  dependencies: &[&registry::ColumnDependency],
  product_deployed: bool,
  present: &HashSet<(String, String)>,
) -> DefinitionState {
  let mut missing_tables: Vec<String> = if product_deployed {
    let tables: HashSet<&str> = present.iter().map(|(table, _)| table.as_str()).collect();
    dependencies
      .iter()
      .map(|dependency| dependency.table.clone())
      .filter(|table| !tables.contains(table.as_str()))
      .collect()
  } else {
    vec![format!("<{product} is not deployed here>")]
  };
  missing_tables.sort();
  missing_tables.dedup();

  if !missing_tables.is_empty() {
    return DefinitionState::Absent { missing_tables };
  }

  let mut missing_columns: Vec<String> = dependencies
    .iter()
    .filter(|dependency| !present.contains(&(dependency.table.clone(), dependency.column.clone())))
    .map(|dependency| format!("{}.{}", dependency.table, dependency.column))
    .collect();
  missing_columns.sort();
  missing_columns.dedup();

  if missing_columns.is_empty() {
    DefinitionState::Active
  } else {
    DefinitionState::Drift { missing_columns }
  }
}

fn render(predicate: SimpleExpr) -> String {
  let sql = Query::select().expr(predicate).take().to_string(PostgresQueryBuilder);
  sql.strip_prefix("SELECT ").unwrap_or(&sql).to_owned()
}

async fn live_columns<C: ConnectionTrait>(db: &C) -> Result<HashSet<(String, String)>, AuthzError> {
  #[derive(FromQueryResult)]
  struct Column {
    table_name:  String,
    column_name: String,
  }

  let rows = Column::find_by_statement(Statement::from_string(
    db.get_database_backend(),
    "SELECT table_name::text AS table_name, column_name::text AS column_name \
     FROM information_schema.columns WHERE table_schema = current_schema()"
      .to_owned(),
  ))
  .all(db)
  .await?;

  Ok(rows.into_iter().map(|row| (row.table_name, row.column_name)).collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionState {
  Active,
  /// A table the rule reads is not in this database at all, so the rule cannot run here — and
  /// should not: sumida is one working tree shared by four products, and its own board and
  /// membership features are optional per deployment, so "no table" is a deployment fact rather
  /// than a defect.
  Absent {
    missing_tables: Vec<String>,
  },
  /// Every table the rule reads is present and a **column** is not. That is a rule and a schema
  /// disagreeing, which `boot_or_abort` treats as fatal: the alternative is discovering it as a
  /// 500 on the first request that evaluates the rule.
  Drift {
    missing_columns: Vec<String>,
  },
}

#[derive(Debug, Clone)]
pub struct DefinitionStatus {
  pub definition: String,
  pub product:    &'static str,
  pub state:      DefinitionState,
}

#[derive(Debug, Clone)]
pub struct BootReport {
  pub entries: Vec<DefinitionStatus>,
}

impl BootReport {
  pub fn active(&self) -> impl Iterator<Item = &DefinitionStatus> {
    self.entries.iter().filter(|e| e.state == DefinitionState::Active)
  }

  pub fn drifted(&self) -> impl Iterator<Item = &DefinitionStatus> {
    self.entries.iter().filter(|e| matches!(e.state, DefinitionState::Drift { .. }))
  }

  pub fn state_of(&self, definition: &str) -> Option<&DefinitionState> {
    self.entries.iter().find(|e| e.definition == definition).map(|e| &e.state)
  }
}

#[cfg(test)]
mod boot_tests {
  use super::*;
  use crate::authz::registry::ColumnDependency;

  fn dependency(table: &str, column: &str) -> ColumnDependency {
    ColumnDependency {
      definition: "thing".to_owned(),
      table:      table.to_owned(),
      column:     column.to_owned(),
    }
  }

  fn database(columns: &[(&str, &str)]) -> HashSet<(String, String)> {
    columns.iter().map(|(t, c)| ((*t).to_owned(), (*c).to_owned())).collect()
  }

  /// Every table and column present — the rule can run here.
  #[test]
  fn a_backed_rule_is_active() {
    let dependencies = [dependency("things", "id"), dependency("things", "user_id")];
    let state = classify(
      "sumida",
      &dependencies.iter().collect::<Vec<_>>(),
      true,
      &database(&[("things", "id"), ("things", "user_id")]),
    );
    assert_eq!(state, DefinitionState::Active);
  }

  /// The product is not deployed here at all. Not an error: sumida is one working tree shared by
  /// four products and no database has all four's tables.
  #[test]
  fn a_rule_of_an_undeployed_product_is_absent() {
    let dependencies = [dependency("tasks", "owner_key")];
    let state = classify("trainer", &dependencies.iter().collect::<Vec<_>>(), false, &database(&[]));
    assert!(matches!(state, DefinitionState::Absent { .. }), "{state:?}");
  }

  /// The product is deployed and one of the rule's tables is not. This is sumida's optional board
  /// feature in a database that does not enable it — site-app is exactly that — so it must read as a
  /// deployment fact and not as drift, or site-app would refuse to start.
  #[test]
  fn a_rule_whose_table_is_missing_is_absent_not_drift() {
    let dependencies = [dependency("boards", "id"), dependency("boards", "group_id")];
    let state =
      classify("sumida", &dependencies.iter().collect::<Vec<_>>(), true, &database(&[("users", "id")]));
    assert_eq!(
      state,
      DefinitionState::Absent {
        missing_tables: vec!["boards".to_owned()],
      }
    );
  }

  /// The table is here and a **column** is not. That is a rule and a schema disagreeing, and it is
  /// the case `boot_or_abort` refuses to start on — the alternative is meeting it as a 500 on the
  /// first request that evaluates the rule.
  #[test]
  fn a_rule_whose_column_is_missing_is_drift() {
    let dependencies = [dependency("tasks", "id"), dependency("tasks", "owner_user_id")];
    let state = classify(
      "trainer",
      &dependencies.iter().collect::<Vec<_>>(),
      true,
      &database(&[("tasks", "id"), ("tasks", "owner_key")]),
    );
    assert_eq!(
      state,
      DefinitionState::Drift {
        missing_columns: vec!["tasks.owner_user_id".to_owned()],
      }
    );
  }
}
