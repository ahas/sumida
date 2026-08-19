//! Relational Zanzibar — the evaluator.
//!
//! **This is the authority, and it is the only one.** Both tiers answer from here: the route gate
//! through `layer::service_ap` and the object tier through every service's `select` /
//! `Authz::try_check`. The lateral-join evaluator over `access_policies` that preceded it is
//! deleted, and nothing on a request path reads that table.
//!
//! **sumida carries its own rules and nobody else's.** A product built on it hands its rule files
//! to `Authz::install_rules` before anything reads the registry; the rule set is then sumida's file
//! plus whatever was installed. Nothing in this module enumerates products or names one.
//!
//! ```text
//! schema.sz + installed  ──parse──▶  ir  ──validate──▶  registry  ──compile──▶  one SQL predicate
//!                                         │                                        │
//!                                         └── aborts boot, naming the offender      └── drops into
//!                                                                                an existing WHERE
//! ```
//!
//! * `subject`  — one identity expansion, replacing four copies and a hand-parser.
//! * `ir`       — the rewrite representation.
//! * `parse`    — the `.sz` rule language of §4.1.
//! * `registry` — the rule set, memoised, with the namespace inventory, and the install seam.
//! * `validate` — the eight boot rules. Failure aborts start-up.
//! * `compile`  — a rewrite becomes one boolean `SimpleExpr`.
//! * `write`    — the grants themselves: write, revoke, and "who holds this on this object".
//! * `effective` — what one caller may do, for `/me/permissions`.
//! * `schema_view` — the vocabulary, for `GET /authz/schema`.

pub mod compile;
pub mod effective;
pub mod ir;
pub mod parse;
pub mod registry;
pub mod schema_view;
pub mod subject;
pub mod validate;
pub mod write;

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
/// Three variants rather than a string, because the id reaches columns of three different types and
/// Postgres has no implicit conversion between any two of them:
///
/// * `relation_tuples.resource_id` is `text`,
/// * `org_closure.descendant_id` is `integer`,
/// * a uuid-keyed definition's own key column (`files.uuid`, `tasks.id`) is `uuid`.
///
/// A point check compares the id against the definition's **own** key column before it ever reaches
/// a tuple, so an id collapsed into text is not a slow query — it is `operator does not exist:
/// uuid = text` at run time, on exactly the uuid-keyed definitions (`file`, `board_file`) whose
/// regressions this repository cares most about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectKey {
  Int(i32),
  Uuid(uuid::Uuid),
  Text(String),
}

impl ObjectKey {
  /// Reads a caller-supplied id string as the type the definition's own key column has.
  ///
  /// Needed wherever an id arrives as text and the definition is not text-keyed — the tuple table
  /// (`resource_id` is `text`), a URL path segment, a JSON body. `Authz::write` is the case that
  /// found this: it took `TupleWrite::resource_id`, a `String`, straight into the `#grant` point
  /// check, which then compared `organizations.id` against a text literal and failed the request
  /// with `operator does not exist: integer = text`. A text-keyed definition cannot see the defect
  /// at all, which is why it went unnoticed until an integer-keyed one was written through.
  pub fn typed(kind: ir::IdKind, definition: &str, id: &str) -> Result<Self, AuthzError> {
    let malformed =
      |what: &str| AuthzError::Schema(format!("`{definition}` is keyed by {what}, and `{id}` is not one"));

    match kind {
      ir::IdKind::Int => id.parse().map(ObjectKey::Int).map_err(|_| malformed("an integer")),
      ir::IdKind::Uuid => id.parse().map(ObjectKey::Uuid).map_err(|_| malformed("a uuid")),
      ir::IdKind::Text => Ok(ObjectKey::Text(id.to_owned())),
    }
  }

  pub(crate) fn as_expr(&self) -> SimpleExpr {
    match self {
      ObjectKey::Int(value) => Expr::val(*value).into(),
      ObjectKey::Uuid(value) => Expr::val(*value).into(),
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
    ObjectKey::Uuid(value)
  }
}

/// The entry points. Deliberately small: everything else is a rule.
pub struct Authz;

impl Authz {
  /// Registers a product's own rule files, before anything reads the rule set.
  ///
  /// sumida ships its own rules and knows of no others; this is how a product says what its own
  /// are. It must run **above** `boot_or_abort` — install after the first read and the process has
  /// already answered questions without them, which is why that is an error and not a warning.
  ///
  /// A product with no rules of its own passes `&[]`, deliberately rather than by omission: "I have
  /// none" and "I forgot" must not look the same.
  pub fn install_rules(files: &'static [registry::RuleFile]) -> Result<(), AuthzError> {
    Registry::install(files)
  }

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

  /// A definition's object id, read out of a string in the type its key column has.
  ///
  /// The definitions with no backing table (`service`, `platform`, `delegation_scope`) are named by
  /// string alone, so they are `Text` and this is a no-op for them.
  pub fn object_key(definition: &str, id: &str) -> Result<ObjectKey, AuthzError> {
    let kind = Self::registry()?.require(definition)?.table.as_ref().map(|table| table.kind);
    ObjectKey::typed(kind.unwrap_or(ir::IdKind::Text), definition, id)
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

  /// A check on one of the string-named singletons — `service`, `platform`, `delegation_scope`
  /// (design §8.2).
  ///
  /// Mechanically `check` with a text id; it exists as its own name because those three have no
  /// row behind them and reading `check("platform", "root", …)` as a row lookup is the mistake
  /// worth spending a function on.
  pub async fn check_object<C: ConnectionTrait>(
    definition: &str,
    id: &str,
    permission: &str,
    subject: &SubjectSet,
    db: &C,
  ) -> Result<bool, AuthzError> {
    Self::check(definition, id, permission, subject, db).await
  }

  /// The service-tier grants one subject holds, in the shape `/me/access-policies` returns
  /// (design §9.7).
  ///
  /// One row per namespace with the relations aggregated into the legacy `action` array, which is
  /// what `updateAbilityFor` (`casl/src/runtime/plugin.ts:19`) consumes — it builds
  /// `can(action, `${namespace}:${object}`)` per element, so the payload has to keep both the
  /// namespace strings and the literal `"service"` object. Service object ids *are* today's
  /// namespace strings, so the projection is exact rather than approximate.
  ///
  /// Two things it deliberately does not do:
  ///
  /// * it does not project the object tier. Those grants are still `access_policies` rows on the
  ///   legacy path (§11 Phase 1), and synthesising them from tuples would report a permission the
  ///   evaluator does not yet consult.
  /// * it does not invent an `effect: "deny"` row. Deny is a rule construct now, not a record, and
  ///   the client's `updateAbilityFor` would turn a synthesised one into a `cannot(...)` nobody
  ///   granted.
  pub async fn service_grants<C: ConnectionTrait>(
    subject: &SubjectSet,
    db: &C,
  ) -> Result<Vec<ServiceGrant>, AuthzError> {
    if subject.is_empty() {
      return Ok(Vec::new());
    }

    ServiceGrant::find_by_statement(Statement::from_sql_and_values(
      db.get_database_backend(),
      // `relation` is the second column of `idx_rt__lookup`, so this is an index range scan per
      // subject key rather than the `@>` a btree cannot answer.
      r#"
      SELECT rt.resource_id AS namespace, array_agg(DISTINCT rt.relation) AS action
        FROM relation_tuples rt
       WHERE rt.resource_type = 'service'
         AND rt.subject_key = ANY($1)
         AND (rt.expires_at IS NULL OR rt.expires_at > CURRENT_TIMESTAMP)
       GROUP BY rt.resource_id
       ORDER BY rt.resource_id
      "#,
      [subject.keys().to_vec().into()],
    ))
    .all(db)
    .await
    .map_err(Into::into)
  }

  /// Whether this subject is a platform administrator. R1, and the only caller-visible remnant of
  /// `session.profile.is_admin` on an authorization path.
  pub async fn is_superuser<C: ConnectionTrait>(subject: &SubjectSet, db: &C) -> Result<bool, AuthzError> {
    Self::check_object("platform", "root", "superuser", subject, db).await
  }

  /// The point-check predicate as a boolean expression, for a caller that wants to place it in a
  /// `WHERE` of its own rather than run it as a statement.
  ///
  /// `ap.rs`'s shim needs exactly this: a route gate is session-constant, so the same expression
  /// serves `SELECT <it>`, `EXISTS (<query> WHERE <it>)` and a filter — which is what lets the
  /// legacy `apply` / `can_access` / `check_allowed` trio keep their three different shapes while
  /// answering from one rule.
  pub fn object_predicate<K: Into<ObjectKey>>(
    definition: &str,
    id: K,
    permission: &str,
    subject: &SubjectSet,
  ) -> Result<SimpleExpr, AuthzError> {
    let registry = Self::registry()?;
    Compiler::new(registry, subject).object_predicate(definition, permission, &id.into())
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
  /// * **Absent** — a table the rule reads is not in this database at all. Not an error: sumida's
  ///   board, file and membership features are optional per deployment, and a feature that is off
  ///   never created its tables, while its rules are compiled in unconditionally.
  /// * **Drift** — every table is present and a **column** the rule reads is missing. That is a
  ///   rule and a schema disagreeing, which is fatal for anything that would evaluate the rule;
  ///   `boot_or_abort` treats it as such.
  pub async fn boot<C: ConnectionTrait>(db: &C) -> Result<BootReport, AuthzError> {
    let registry = Self::registry()?; // parse + validate; aborts here on any structural failure
    let present = live_columns(db).await?;

    let dependencies = registry.column_dependencies();
    let entries = registry
      .definitions()
      .iter()
      .map(|definition| {
        // Not just this definition's own columns: a rule that walks (`board->view`) emits a
        // subquery over the target's table, so a column missing two hops away breaks this
        // definition's statement just as surely as one of its own. Judging by own columns alone
        // reported a definition backed while a column two hops away did not exist.
        let reachable = registry.reachable_from(&definition.name);
        let mine: Vec<&registry::ColumnDependency> = dependencies
          .iter()
          .filter(|dependency| reachable.contains(&dependency.definition.as_str()))
          .collect();
        DefinitionStatus {
          definition: definition.name.clone(),
          source:     registry.source_of(&definition.name).unwrap_or(registry::SUMIDA_RULES.name),
          state:      classify(&mine, &present),
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
  /// The projection is never read by the evaluator. It exists for the foreign key on
  /// `relation_tuples`, which is the only thing that can stop a caller writing a tuple on a
  /// relation nobody declared — and for `GET /authz/schema`. It is therefore part of the boot
  /// sequence: a rule may declare a relation before any tuple names it, and the row has to be
  /// there before the first grant.
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
/// draws is the whole value of the check: **Absent** is a deployment fact — a switched-off optional
/// feature never created its tables, and the rules that read them are compiled in regardless,
/// because `.sz` has no conditional compilation — while **Drift** is a rule and a schema
/// disagreeing, which fails closed only by accident, as a 500 on the first request that evaluates
/// the rule.
fn classify(
  dependencies: &[&registry::ColumnDependency],
  present: &HashSet<(String, String)>,
) -> DefinitionState {
  let tables: HashSet<&str> = present.iter().map(|(table, _)| table.as_str()).collect();
  let mut missing_tables: Vec<String> = dependencies
    .iter()
    .map(|dependency| dependency.table.clone())
    .filter(|table| !tables.contains(table.as_str()))
    .collect();
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

pub(crate) fn render(predicate: SimpleExpr) -> String {
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

/// One namespace's service-tier grants, as `/me/access-policies` reports them.
#[derive(Debug, Clone, FromQueryResult)]
pub struct ServiceGrant {
  /// The legacy namespace string — which is literally the `service` object's id.
  pub namespace: String,
  pub action:    Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionState {
  Active,
  /// A table the rule reads is not in this database at all, so the rule cannot run here — and
  /// should not. sumida's board, file and membership features are optional per deployment and a
  /// feature that is off never created its tables, while the rules that read them are compiled in
  /// unconditionally, so "no table" is a deployment fact rather than a defect.
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
  /// The rule file this definition was declared in. Report-only.
  pub source:     &'static str,
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
      &dependencies.iter().collect::<Vec<_>>(),
      &database(&[("things", "id"), ("things", "user_id")]),
    );
    assert_eq!(state, DefinitionState::Active);
  }

  /// One of the rule's tables is not here. This is an optional feature in a database that does not
  /// enable it — the rules for `board`, `file` and `membership_plan` are compiled in whether or not
  /// the feature that creates their tables is on, because the rule language has no conditional
  /// compilation. So a missing **table** has to read as a deployment fact and not as drift, or
  /// every deployment with a feature switched off refuses to start.
  ///
  /// This is the executable form of the whole feature-flag argument, and it is the reason there is
  /// no per-product marker table any more: a product that ships only its own rules has no foreign
  /// rules to suppress, and the branch that made an absent optional feature survivable was never
  /// the marker.
  #[test]
  fn a_rule_whose_table_is_missing_is_absent_not_drift() {
    let dependencies = [dependency("boards", "id"), dependency("boards", "group_id")];
    let state = classify(&dependencies.iter().collect::<Vec<_>>(), &database(&[("users", "id")]));
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
    let dependencies = [dependency("things", "id"), dependency("things", "owner_user_id")];
    let state = classify(
      &dependencies.iter().collect::<Vec<_>>(),
      &database(&[("things", "id"), ("things", "owner_key")]),
    );
    assert_eq!(
      state,
      DefinitionState::Drift {
        missing_columns: vec!["things.owner_user_id".to_owned()],
      }
    );
  }
}
