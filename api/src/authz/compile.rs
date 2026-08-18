//! The compiler: a rewrite rule becomes **one boolean SQL predicate** (design §6).
//!
//! This is the property the whole design rests on. `ap.rs::with_lateral_join` aggregates
//! `bool_or(effect='allow') / bool_or(effect='deny')` per row in a `LEFT JOIN LATERAL`, which keeps
//! filter, `COUNT`, `ORDER BY` and `LIMIT/OFFSET` in one statement; a Zanzibar *service* would
//! answer with an id set that has to be fed back into SQL, and that breaks pagination at trainer's
//! 15 000 tasks and lab-app's 13 647 photos. So a rule compiles to a `SimpleExpr` that drops into an
//! existing `WHERE`, and nothing here ever materialises an id list into Rust.
//!
//! With per-tuple `deny` gone there is nothing left to aggregate, so the lateral goes too:
//!
//! | construct | SQL |
//! |---|---|
//! | stored `relation`                     | `EXISTS (SELECT 1 FROM relation_tuples …)` |
//! | derived relation, own-table column    | `<col> = ANY($keys)` / `= ANY($user_ids)` — no join |
//! | derived relation, `of` / uncorrelated | `EXISTS (SELECT 1 FROM <table> …)` |
//! | derived relation, one or more hops    | nested `EXISTS (SELECT 1 FROM <hop table> …)` |
//! | `permission`                          | inlined |
//! | `rel^`                                | `<id> IN (SELECT descendant_id FROM relation_tuples JOIN <closure> …)`, uncorrelated so Postgres hoists it into an InitPlan |
//! | `rel->perm`                           | the sub-expression, rewritten against the target object's id |
//! | <code>a &#124; b</code>               | `(a OR b)` |
//! | `a & b`                               | `(a AND b)` |
//! | `a - b`                               | `(a) AND NOT (b)`, and `b` may be a union |
//! | union of same-shape tuple branches    | flattened into one `relation IN (…)` |
//!
//! Every tuple predicate ANDs `(expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)`.
//!
//! It also removed a defect the old design had. `organization_ancestors_subquery` hardcoded the
//! alias `"ap"."object"`, which is why `project_ap.rs` carried a warning that the point check and
//! the list check could not be built from one expression. Here they compile from the same tree, so
//! they cannot diverge.

use std::cell::Cell;
use std::collections::HashSet;

use sea_orm::JoinType;
use sea_orm::sea_query::{Alias, Expr, Query, SelectStatement, SimpleExpr};

use super::ir::*;
use super::registry::Registry;
use super::subject::{PUBLIC_WILDCARD, SubjectSet};
use super::{AuthzError, ObjectKey};

const TUPLES: &str = "relation_tuples";

fn alias(name: &str) -> Alias {
  Alias::new(name)
}

fn column(table: &str, name: &str) -> Expr {
  Expr::col((alias(table), alias(name)))
}

/// One object id, carried in whichever form the next predicate needs.
///
/// The distinction is load-bearing rather than cosmetic: `relation_tuples.resource_id` is `text`
/// while `org_closure.descendant_id` is `integer`, so the same id reaches two columns of different
/// types. Keeping the integer-ness in the type means the cast lands exactly where it is needed, and
/// `^` on a text-keyed definition is a compile error instead of a runtime one.
#[derive(Clone, Debug)]
enum Id {
  Int(SimpleExpr),
  /// A uuid, or anything else that is not text and must be cast to reach `resource_id`.
  Cast(SimpleExpr),
  Text(SimpleExpr),
}

impl Id {
  fn of(def: &Definition, expr: SimpleExpr) -> Self {
    match &def.table {
      Some(table) if table.kind.is_int() => Id::Int(expr),
      Some(table) if table.kind.needs_text_cast() => Id::Cast(expr),
      _ => Id::Text(expr),
    }
  }

  fn as_text(&self) -> SimpleExpr {
    match self {
      Id::Int(e) | Id::Cast(e) => e.clone().cast_as(alias("text")),
      Id::Text(e) => e.clone(),
    }
  }

  fn as_int(&self, what: &str) -> Result<SimpleExpr, AuthzError> {
    match self {
      Id::Int(e) => Ok(e.clone()),
      _ => Err(AuthzError::NotCompilable(format!(
        "{what} needs an integer object id, but the definition's id is text"
      ))),
    }
  }
}

/// The definition being judged, the alias its backing table is reachable under, and the object id.
struct Scope<'a> {
  def:   &'a Definition,
  /// `None` when the definition has no backing table, or when nothing in the rule needs a column
  /// from it. A rule that then asks for one is a compile error rather than broken SQL.
  alias: Option<String>,
  id:    Id,
}

impl Scope<'_> {
  /// The object id in whichever form this definition's own columns compare against.
  fn native_id(&self) -> Result<SimpleExpr, AuthzError> {
    match &self.def.table {
      Some(table) if table.kind.is_int() => self.id.as_int(&self.def.name),
      _ => Ok(self.id.as_text()),
    }
  }
}

/// How a union branch groups with its neighbours.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Group {
  /// Stored relations of this definition, partitioned by wildcard admission: merging the two would
  /// let a relation that never declared `user:*` match a wildcard tuple.
  Stored(bool),
  Closure,
}

pub struct Compiler<'a> {
  registry: &'a Registry,
  subject:  &'a SubjectSet,
  seq:      Cell<u32>,
}

impl<'a> Compiler<'a> {
  pub fn new(registry: &'a Registry, subject: &'a SubjectSet) -> Self {
    Self {
      registry,
      subject,
      seq: Cell::new(0),
    }
  }

  /// The predicate for a list filter: the object is a row of the definition's own table, referred
  /// to by the table name — which is how SeaORM renders `Select<E>`'s columns, so the predicate
  /// drops straight into that query's `WHERE` with nothing else touched.
  pub fn filter_predicate(&self, definition: &str, permission: &str) -> Result<SimpleExpr, AuthzError> {
    let def = self.registry.require(definition)?;
    let Some(table) = &def.table else {
      return Err(AuthzError::NotCompilable(format!(
        "`{definition}` has no backing table, so it cannot filter a query; use a point check"
      )));
    };
    let scope = Scope {
      def,
      alias: Some(table.table.clone()),
      id: Id::of(def, column(&table.table, &table.id_col).into()),
    };
    self.rewrite(self.permission_of(def, permission)?, &scope)
  }

  /// The predicate for a point check on one object id.
  pub fn object_predicate(
    &self,
    definition: &str,
    permission: &str,
    id: &ObjectKey,
  ) -> Result<SimpleExpr, AuthzError> {
    let def = self.registry.require(definition)?;
    let rewrite = self.permission_of(def, permission)?;
    self.at_object(definition, id.as_expr(), rewrite)
  }

  /// `SELECT EXISTS (…)` — the statement a point check runs.
  pub fn object_check_statement(
    &self,
    definition: &str,
    permission: &str,
    id: &ObjectKey,
  ) -> Result<SelectStatement, AuthzError> {
    let predicate = self.object_predicate(definition, permission, id)?;
    Ok(Query::select().expr_as(predicate, alias("allowed")).take())
  }

  // ── plumbing ──────────────────────────────────────────────────────────────────────────────

  fn fresh(&self, prefix: &str) -> String {
    let n = self.seq.get();
    self.seq.set(n + 1);
    format!("_az_{prefix}{n}")
  }

  fn permission_of<'d>(&self, def: &'d Definition, name: &str) -> Result<&'d Rewrite, AuthzError> {
    def.permission(name).ok_or_else(|| AuthzError::UnknownPermission {
      definition: def.name.clone(),
      permission: name.to_owned(),
    })
  }

  /// `$1 = ANY($2)` written out, because sea-query renders the composed form as
  /// `col = (ANY(...))`, which is not valid PostgreSQL — the same trap `util/org.rs:31` documents.
  /// The array is one bound parameter, so the plan shape does not change with the caller's identity.
  fn any_of_text(&self, expr: SimpleExpr, values: Vec<String>) -> SimpleExpr {
    Expr::cust_with_exprs("$1 = ANY($2)", [expr, Expr::val(values).cast_as(alias("text[]"))])
  }

  fn any_of_int(&self, expr: SimpleExpr, values: Vec<i32>) -> SimpleExpr {
    Expr::cust_with_exprs("$1 = ANY($2)", [expr, Expr::val(values).cast_as(alias("int[]"))])
  }

  fn subject_match(&self, tuple_alias: &str, allows_wildcard: bool) -> SimpleExpr {
    let key = column(tuple_alias, "subject_key");
    let matched = self.any_of_text(key.clone().into(), self.subject.keys().to_vec());
    if allows_wildcard { matched.or(key.eq(PUBLIC_WILDCARD)) } else { matched }
  }

  /// Matches a column that holds an integer user id.
  fn user_match(&self, expr: SimpleExpr) -> SimpleExpr {
    self.any_of_int(expr, self.subject.user_ids().to_vec())
  }

  /// Matches a column that already holds a canonical subject key (`tasks.owner_key`).
  fn subject_key_match(&self, expr: SimpleExpr) -> SimpleExpr {
    self.any_of_text(expr, self.subject.keys().to_vec())
  }

  fn unexpired(tuple_alias: &str) -> SimpleExpr {
    let expires = column(tuple_alias, "expires_at");
    expires.clone().is_null().or(expires.gt(Expr::cust("CURRENT_TIMESTAMP")))
  }

  fn relation_match(tuple_alias: &str, relations: &[&str]) -> SimpleExpr {
    let col = column(tuple_alias, "relation");
    if relations.len() == 1 {
      col.eq(relations[0])
    } else {
      col.is_in(relations.iter().copied())
    }
  }

  /// `EXISTS (SELECT 1 FROM relation_tuples …)` for one or more relations of the same shape.
  fn tuple_exists(
    &self,
    resource_type: &str,
    relations: &[&str],
    resource_id: SimpleExpr,
    allows_wildcard: bool,
  ) -> SimpleExpr {
    let a = self.fresh("t");
    Expr::exists(
      Query::select()
        .expr(Expr::val(1))
        .from_as(alias(TUPLES), alias(&a))
        .and_where(column(&a, "resource_type").eq(resource_type))
        .and_where(Self::relation_match(&a, relations))
        .and_where(column(&a, "resource_id").eq(resource_id))
        .and_where(self.subject_match(&a, allows_wildcard))
        .and_where(Self::unexpired(&a))
        .take(),
    )
  }

  /// `<id> IN (SELECT descendant_id …)` — the `^` closure (E3).
  ///
  /// The subquery is **uncorrelated**: the subject side is session-constant, so Postgres hoists it
  /// into a hashed InitPlan and the per-row cost is one integer probe. It has the same shape as
  /// `managed_org_ids` (`service/membership/mod.rs:72`) and none of its properties — it is
  /// generated from the same rewrite tree as the point check so the two cannot diverge, it stays
  /// inside the statement so pagination is unaffected, and exclusion is applied by the rule outside
  /// the union so a block cannot be lost.
  fn closure_exists(
    &self,
    resource_type: &str,
    relations: &[&str],
    closure_table: &str,
    id: SimpleExpr,
  ) -> SimpleExpr {
    let m = self.fresh("m");
    let c = self.fresh("c");

    let statement = Query::select()
      .expr(column(&c, "descendant_id"))
      .from_as(alias(TUPLES), alias(&m))
      .join_as(
        JoinType::InnerJoin,
        alias(closure_table),
        alias(&c),
        Expr::col((alias(&c), alias("ancestor_id"))).equals((alias(&m), alias("resource_id_int"))),
      )
      .and_where(column(&m, "resource_type").eq(resource_type))
      .and_where(Self::relation_match(&m, relations))
      .and_where(self.subject_match(&m, false))
      .and_where(Self::unexpired(&m))
      .take();

    Expr::expr(id).in_subquery(statement)
  }

  // ── the rewrite walk ──────────────────────────────────────────────────────────────────────

  fn rewrite(&self, rewrite: &Rewrite, scope: &Scope<'_>) -> Result<SimpleExpr, AuthzError> {
    match rewrite {
      Rewrite::Union(_) => self.union(rewrite, scope),
      // A bare name is a relation or a permission; only the definition knows which.
      Rewrite::This(name) | Rewrite::Computed(name) => {
        if scope.def.relation(name).is_some() {
          self.term(name, scope)
        } else {
          self.union(rewrite, scope)
        }
      }
      Rewrite::Closure(relation) => self.closure(relation, scope),
      Rewrite::Walk { edge, next } => self.walk(edge, next, scope),
      Rewrite::Absolute { ty, id, next } => self.absolute(ty, id, next),
      Rewrite::Intersection(parts) => {
        let mut out: Option<SimpleExpr> = None;
        for part in parts {
          let compiled = self.rewrite(part, scope)?;
          out = Some(match out {
            Some(acc) => acc.and(compiled),
            None => compiled,
          });
        }
        out.ok_or_else(|| AuthzError::NotCompilable("an empty intersection".to_owned()))
      }
      Rewrite::Exclusion { base, subtract } => {
        let base = self.rewrite(base, scope)?;
        let subtract = self.rewrite(subtract, scope)?;
        Ok(base.and(subtract.not()))
      }
    }
  }

  /// A union, flattened. `can_read = read | operate_read` with
  /// `operate_read = op_read | operator | platform:root->superuser` collapses into one
  /// `relation IN ('read','op_read','operator')` plus the platform branch, instead of three
  /// separate `EXISTS` over the same index.
  fn union(&self, rewrite: &Rewrite, scope: &Scope<'_>) -> Result<SimpleExpr, AuthzError> {
    let mut members = Vec::new();
    self.expand(rewrite, scope, &mut HashSet::new(), &mut members)?;

    let group_of = |member: &Rewrite| -> Option<Group> {
      match member {
        Rewrite::This(name) | Rewrite::Computed(name) => match scope.def.relation(name) {
          Some(Relation {
            source: RelationSource::Stored {
              allows_wildcard, ..
            },
            ..
          }) => Some(Group::Stored(*allows_wildcard)),
          _ => None,
        },
        Rewrite::Closure(_) => Some(Group::Closure),
        _ => None,
      }
    };
    let names_in = |group: Group| -> Vec<&str> {
      members
        .iter()
        .filter(|m| group_of(m) == Some(group))
        .filter_map(|m| match m {
          Rewrite::This(name) | Rewrite::Computed(name) | Rewrite::Closure(name) => Some(name.as_str()),
          _ => None,
        })
        .collect()
    };

    // Order is preserved: each group is emitted where its first member sat, so the cheap branches
    // stay in front of the expensive ones and the `OR` short-circuits on the common case.
    let mut emitted: HashSet<Group> = HashSet::new();
    let mut out: Option<SimpleExpr> = None;
    for member in &members {
      let compiled = match group_of(member) {
        Some(group) if !emitted.insert(group) => continue,
        Some(Group::Stored(wildcard)) => self.tuple_exists(
          &scope.def.name,
          &names_in(Group::Stored(wildcard)),
          scope.id.as_text(),
          wildcard,
        ),
        Some(Group::Closure) => {
          let Some(closure) = &scope.def.closure else {
            return Err(AuthzError::NotCompilable(format!(
              "`{}` uses `^` but declares no closure table",
              scope.def.name
            )));
          };
          let id = scope.id.as_int(&format!("`{}`'s closure", scope.def.name))?;
          self.closure_exists(&scope.def.name, &names_in(Group::Closure), &closure.table, id)
        }
        None => self.rewrite(member, scope)?,
      };
      out = Some(match out {
        Some(acc) => acc.or(compiled),
        None => compiled,
      });
    }

    out.ok_or_else(|| AuthzError::NotCompilable("an empty union".to_owned()))
  }

  /// Inlines computed permissions and splices nested unions, so the grouping above sees every
  /// branch of the whole union at once. An inlined permission that is *not* a union (an exclusion,
  /// an intersection) stays one term — union is associative, exclusion is not.
  fn expand(
    &self,
    rewrite: &Rewrite,
    scope: &Scope<'_>,
    visiting: &mut HashSet<String>,
    out: &mut Vec<Rewrite>,
  ) -> Result<(), AuthzError> {
    match rewrite {
      Rewrite::Union(children) => {
        for child in children {
          self.expand(child, scope, visiting, out)?;
        }
        Ok(())
      }
      Rewrite::This(name) | Rewrite::Computed(name)
        if scope.def.relation(name).is_none() && scope.def.permission(name).is_some() =>
      {
        // Acyclicity is a boot-validated property; this guard is here so a schema that somehow
        // slipped past validation fails with a name instead of a stack overflow.
        if !visiting.insert(format!("{}#{name}", scope.def.name)) {
          return Err(AuthzError::NotCompilable(format!(
            "permission `{}#{name}` is defined in terms of itself",
            scope.def.name
          )));
        }
        let inner = scope.def.permission(name).expect("checked in the guard").clone();
        self.expand(&inner, scope, visiting, out)?;
        visiting.remove(&format!("{}#{name}", scope.def.name));
        Ok(())
      }
      other => {
        out.push(other.clone());
        Ok(())
      }
    }
  }

  fn closure(&self, relation: &str, scope: &Scope<'_>) -> Result<SimpleExpr, AuthzError> {
    let Some(closure) = &scope.def.closure else {
      return Err(AuthzError::NotCompilable(format!(
        "`{0}#{relation}^` needs a closure table, and `{0}` declares none",
        scope.def.name
      )));
    };
    let id = scope.id.as_int(&format!("`{}#{relation}^`", scope.def.name))?;
    Ok(self.closure_exists(&scope.def.name, &[relation], &closure.table, id))
  }

  /// A bare term: a stored relation, or a derived relation that is a predicate in its own right.
  fn term(&self, name: &str, scope: &Scope<'_>) -> Result<SimpleExpr, AuthzError> {
    let Some(relation) = scope.def.relation(name) else {
      return Err(AuthzError::UnknownRelation {
        definition: scope.def.name.clone(),
        relation:   name.to_owned(),
      });
    };

    match &relation.source {
      RelationSource::Stored {
        allows_wildcard, ..
      } => Ok(self.tuple_exists(&scope.def.name, &[name], scope.id.as_text(), *allows_wildcard)),
      RelationSource::Derived(derivation) => self.derived_term(name, derivation, scope),
    }
  }

  fn derived_term(
    &self,
    name: &str,
    derivation: &Derivation,
    scope: &Scope<'_>,
  ) -> Result<SimpleExpr, AuthzError> {
    if let EdgeTarget::Object(target) = &derivation.target {
      return Err(AuthzError::NotCompilable(format!(
        "`{}#{name}` is an edge to `{target}` and is only usable as `{name}-><permission>`",
        scope.def.name
      )));
    }
    let subject_of = |expr: SimpleExpr| match derivation.target {
      EdgeTarget::SubjectKey => self.subject_key_match(expr),
      _ => self.user_match(expr),
    };

    match &derivation.correlation {
      // The column lives on the row under test, so there is no join at all.
      Correlation::Own => {
        let table = self.own_alias(scope, name)?;
        let matched = subject_of(column(&table, &derivation.column).into());
        Ok(self.with_filter(matched, derivation, &table))
      }
      // The userset direction: the rows of `table` whose `of` column is this object.
      Correlation::Of(of_column) => {
        let a = self.fresh("m");
        let subject = subject_of(column(&a, &derivation.column).into());
        Ok(Expr::exists(
          Query::select()
            .expr(Expr::val(1))
            .from_as(alias(&derivation.table), alias(&a))
            .and_where(column(&a, of_column).eq(scope.native_id()?))
            .and_where(self.with_filter(subject, derivation, &a))
            .take(),
        ))
      }
      // Uncorrelated. `platform#flagged` is the only one, and being uncorrelated is the point:
      // Postgres evaluates it once per statement as an InitPlan, so R1 costs nothing per row — and
      // with a secured API key the bound array is empty, so the branch is statically false.
      Correlation::Free => {
        let a = self.fresh("m");
        let subject = subject_of(column(&a, &derivation.column).into());
        Ok(Expr::exists(
          Query::select()
            .expr(Expr::val(1))
            .from_as(alias(&derivation.table), alias(&a))
            .and_where(self.with_filter(subject, derivation, &a))
            .take(),
        ))
      }
    }
  }

  fn own_alias(&self, scope: &Scope<'_>, name: &str) -> Result<String, AuthzError> {
    scope.alias.clone().ok_or_else(|| {
      AuthzError::NotCompilable(format!(
        "`{0}#{name}` reads a column of `{0}`'s table, which is not in scope here",
        scope.def.name
      ))
    })
  }

  /// ANDs the derivation's `where` clause, rewriting its table qualifier to whatever alias the
  /// table carries here. Validation guarantees the clause qualifies with the derivation's own table
  /// and nothing else, which is what makes this a substitution rather than a guess.
  fn with_filter(&self, expr: SimpleExpr, derivation: &Derivation, table_alias: &str) -> SimpleExpr {
    let Some(filter) = &derivation.filter else { return expr };
    let sql = if table_alias == derivation.table {
      filter.clone()
    } else {
      filter.replace(&format!("{}.", derivation.table), &format!("\"{table_alias}\"."))
    };
    expr.and(Expr::cust(sql))
  }

  fn walk(&self, edge: &str, next: &Rewrite, scope: &Scope<'_>) -> Result<SimpleExpr, AuthzError> {
    let Some(relation) = scope.def.relation(edge) else {
      return Err(AuthzError::UnknownRelation {
        definition: scope.def.name.clone(),
        relation:   edge.to_owned(),
      });
    };
    let RelationSource::Derived(derivation) = &relation.source else {
      return Err(AuthzError::NotCompilable(format!(
        "`{}#{edge}` is a stored relation, so it cannot be walked through",
        scope.def.name
      )));
    };
    let EdgeTarget::Object(target) = &derivation.target else {
      return Err(AuthzError::NotCompilable(format!(
        "`{}#{edge}` resolves to a subject, not an object, so it cannot be walked through",
        scope.def.name
      )));
    };

    let table = self.own_alias(scope, edge)?;

    // The expression entering each hop, and the alias each hop's table takes.
    let mut entering: Vec<SimpleExpr> = vec![column(&table, &derivation.column).into()];
    let mut hop_aliases = Vec::new();
    for hop in &derivation.hops {
      let a = self.fresh("h");
      entering.push(column(&a, &hop.next_col).into());
      hop_aliases.push(a);
    }

    let mut predicate =
      self.at_object(target, entering.last().expect("at least the start").clone(), next)?;

    // Wrap outward: the innermost hop's `EXISTS` is built first and each earlier hop encloses it.
    for index in (0..derivation.hops.len()).rev() {
      let hop = &derivation.hops[index];
      let a = &hop_aliases[index];
      predicate = Expr::exists(
        Query::select()
          .expr(Expr::val(1))
          .from_as(alias(&hop.table), alias(a))
          .and_where(column(a, &hop.key_col).eq(entering[index].clone()))
          .and_where(predicate)
          .take(),
      );
    }

    Ok(self.with_filter(predicate, derivation, &table))
  }

  fn absolute(&self, ty: &str, id: &str, next: &Rewrite) -> Result<SimpleExpr, AuthzError> {
    let def = self.registry.require(ty)?;
    if def.table.is_some() {
      return Err(AuthzError::NotCompilable(format!(
        "`{ty}:{id}` names a row-backed definition; an absolute reference is only for the \
         string-named singletons (`platform`, `service`, `delegation_scope`)"
      )));
    }
    self.at_object(ty, Expr::val(id).into(), next)
  }

  /// Judges `rewrite` against the object of `definition` identified by `id_expr`.
  ///
  /// The wrap decision is the interesting part. When the rule reads a column of the target's own
  /// table the predicate has to run over that row, so it is enclosed in
  /// `EXISTS (SELECT 1 FROM <table> WHERE <id_col> = <id_expr> AND …)`. When it does not — which is
  /// every organization branch, because `manager^` and friends are tuples and closures only — the
  /// id expression is substituted directly and no row is fetched. That is the difference between
  /// §6.2's `cu.organization_id IN (…)` and one extra join per row.
  fn at_object(
    &self,
    definition: &str,
    id_expr: SimpleExpr,
    rewrite: &Rewrite,
  ) -> Result<SimpleExpr, AuthzError> {
    let def = self.registry.require(definition)?;

    if !self.reads_own_table(def, rewrite) {
      let scope = Scope {
        def,
        alias: None,
        id: Id::of(def, id_expr),
      };
      return self.rewrite(rewrite, &scope);
    }

    let table = def.table.as_ref().expect("reads_own_table implies a table");
    let a = self.fresh("o");
    let scope = Scope {
      def,
      alias: Some(a.clone()),
      id: Id::of(def, column(&a, &table.id_col).into()),
    };
    let inner = self.rewrite(rewrite, &scope)?;

    Ok(Expr::exists(
      Query::select()
        .expr(Expr::val(1))
        .from_as(alias(&table.table), alias(&a))
        .and_where(column(&a, &table.id_col).eq(id_expr))
        .and_where(inner)
        .take(),
    ))
  }

  /// Whether evaluating `rewrite` on `def` needs a column of `def`'s own table.
  ///
  /// Computed permissions are followed because they stay on this object; `->` and absolute
  /// references are not, because they leave it and make their own wrap decision.
  fn reads_own_table(&self, def: &Definition, rewrite: &Rewrite) -> bool {
    let Some(table) = &def.table else { return false };
    self.reads_own_table_inner(def, &table.table, rewrite, &mut HashSet::new())
  }

  fn reads_own_table_inner(
    &self,
    def: &Definition,
    table: &str,
    rewrite: &Rewrite,
    seen: &mut HashSet<String>,
  ) -> bool {
    let own_column = |name: &str| {
      def.relation(name).is_some_and(|r| {
        matches!(&r.source,
                 RelationSource::Derived(d) if d.table == table && d.correlation == Correlation::Own)
      })
    };

    match rewrite {
      Rewrite::This(name) | Rewrite::Computed(name) => {
        if def.relation(name).is_some() {
          return own_column(name);
        }
        match def.permission(name) {
          Some(inner) if seen.insert(name.clone()) => self.reads_own_table_inner(def, table, inner, seen),
          _ => false,
        }
      }
      Rewrite::Closure(_) | Rewrite::Absolute { .. } => false,
      // The edge column itself lives on this table.
      Rewrite::Walk { edge, .. } => own_column(edge),
      Rewrite::Union(parts) | Rewrite::Intersection(parts) => {
        parts.iter().any(|p| self.reads_own_table_inner(def, table, p, seen))
      }
      Rewrite::Exclusion { base, subtract } => {
        self.reads_own_table_inner(def, table, base, seen)
          || self.reads_own_table_inner(def, table, subtract, seen)
      }
    }
  }
}
