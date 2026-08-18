//! Boot validation (design §4.4). **Every failure here aborts start-up.**
//!
//! That is the point of the phase. The defects this replaces were all silent: a namespace nobody
//! declared fell through to "no policy matches, deny"; a relation nobody defined was a string that
//! matched nothing; a check nobody wrote was an absent `if`. None of them produced a symptom until
//! somebody was locked out or read something they should not have. Here each of them is a process
//! that will not start, with the offender named.
//!
//! Every rule is checked against the whole registry and **all** the failures are reported at once,
//! not the first — fixing a schema one boot at a time is how a person gives up and loosens the
//! check instead.
//!
//! | rule | what it stops |
//! |---|---|
//! | 1 | a rewrite naming a relation or permission that does not exist |
//! | 2 | a subject vocabulary that admits a tuple nothing can ever match (a reversed `admin:user` row), and a userset that is not O(1)-derivable from the session (E4) |
//! | 3 | a permission defined in terms of itself, directly or through a walk |
//! | 4 | `^` over anything but a stored relation — an exclusion re-evaluated at every ancestor level is not a bounded join |
//! | 5 | a `->` chain deeper than 4 |
//! | 6 | a `column(...)` whose shape the compiler cannot honour |
//! | 8 | a live namespace with no rule behind it |
//! | — | a rule that parses and validates and still does not compile to one predicate |
//!
//! Rule 7 (the `relation_definitions` / `permission_definitions` projection agreeing with the
//! compiled registry) needs a database and lives in `Authz::sync_schema`.

use std::collections::HashSet;

use super::compile::Compiler;
use super::ir::*;
use super::registry::Registry;
use super::subject::SubjectSet;
use super::{AuthzError, ObjectKey};

/// The maximum `->` chain depth (rule 5). A plan-quality limit rather than a correctness one, and
/// rejected at boot so nobody discovers it in production. The deepest chain in the rule set is 3.
const MAX_WALK_DEPTH: usize = 4;

/// The relation the closure tables materialise must live on a definition whose ids are integers,
/// because `org_closure` and `board_group_closure` are integer-keyed.
pub fn validate(registry: &Registry) -> Result<(), AuthzError> {
  let mut errors = Vec::new();

  for definition in registry.definitions() {
    check_relations(registry, definition, &mut errors);
    for (permission, rewrite) in &definition.permissions {
      let at = format!("{}#{permission}", definition.name);
      check_rewrite(registry, definition, rewrite, &at, 0, &mut errors);
    }
  }

  check_cycles(registry, &mut errors);
  check_namespaces(registry, &mut errors);

  // A rule that survives every structural check and still will not compile is exactly the case
  // §6.5 says must be named and refused rather than degraded silently.
  if errors.is_empty() {
    check_compiles(registry, &mut errors);
  }

  if errors.is_empty() { Ok(()) } else { Err(AuthzError::Validation(errors)) }
}

// ── rules 2 and 6: relation declarations ────────────────────────────────────────────────────

fn check_relations(registry: &Registry, definition: &Definition, errors: &mut Vec<String>) {
  if let Some(closure) = &definition.closure {
    match &definition.table {
      Some(table) if table.kind.is_int() => {}
      _ => errors.push(format!(
        "{}: `closure {} via {}` needs integer object ids, because the closure tables are \
         integer-keyed; declare `on <table>.<col> int`",
        definition.name, closure.relation, closure.table
      )),
    }
    if definition.relation(&closure.relation).is_none() {
      errors.push(format!(
        "{}: closure names relation `{}`, which is not declared",
        definition.name, closure.relation
      ));
    }
  }

  for relation in &definition.relations {
    let at = format!("{}#{}", definition.name, relation.name);
    match &relation.source {
      RelationSource::Stored { subjects, .. } => {
        for subject in subjects {
          check_subject_spec(registry, subject, &at, errors);
        }
      }
      RelationSource::Derived(derivation) => {
        check_derivation(registry, definition, derivation, &at, errors)
      }
    }
  }
}

/// Rule 2. A subject type nobody declared is a tuple that can be written and can never match — a
/// reversed `admin:user` row, sitting in the table doing nothing.
fn check_subject_spec(registry: &Registry, spec: &SubjectSpec, at: &str, errors: &mut Vec<String>) {
  let ty = match spec {
    SubjectSpec::Direct(ty) | SubjectSpec::Wildcard(ty) => ty,
    SubjectSpec::Userset { ty, relation } => {
      let Some(def) = registry.get(ty) else {
        errors.push(format!("{at}: subject `{spec}` names undeclared definition `{ty}`"));
        return;
      };
      // E4: a userset subject must be resolvable from the session in O(1), which is what keeps the
      // subject side one string match instead of a recursive resolution.
      match def.relation(relation).map(|r| &r.source) {
        Some(RelationSource::Derived(derivation))
          if derivation.table == "users" && matches!(derivation.correlation, Correlation::Of(_)) => {}
        Some(_) => errors.push(format!(
          "{at}: subject `{spec}` is not derivable from the session — a userset subject must be a \
           `column(users.… of users.…)` relation (extension E4)"
        )),
        None => errors.push(format!(
          "{at}: subject `{spec}` names relation `{relation}`, which `{ty}` \
                                     does not declare"
        )),
      }
      return;
    }
  };

  if registry.get(ty).is_none() {
    errors.push(format!("{at}: subject `{spec}` names undeclared definition `{ty}`"));
  }
}

/// Rule 6, plus the correlation legality the compiler depends on.
fn check_derivation(
  registry: &Registry,
  definition: &Definition,
  derivation: &Derivation,
  at: &str,
  errors: &mut Vec<String>,
) {
  match &derivation.target {
    EdgeTarget::Object(target) => {
      if registry.get(target).is_none() {
        errors.push(format!("{at}: derives `{target}`, which is not declared"));
      }
      if derivation.correlation != Correlation::Own {
        errors.push(format!(
          "{at}: an edge to another definition must read a column of `{}`'s own row, so it cannot \
           use `of` and cannot sit on a definition with no table",
          definition.name
        ));
      }
    }
    EdgeTarget::User | EdgeTarget::SubjectKey => {
      if !derivation.hops.is_empty() {
        errors.push(format!("{at}: `via` is only meaningful for an edge to another definition"));
      }
    }
  }

  match (&derivation.correlation, &definition.table) {
    (Correlation::Own, Some(table)) if table.table != derivation.table => errors.push(format!(
      "{at}: reads `{}`, which is not `{}`'s own table (`{}`); use `of <table>.<column>` to \
       correlate it to this object",
      derivation.table, definition.name, table.table
    )),
    (Correlation::Own, None) => errors.push(format!(
      "{at}: `{}` has no backing table, so a derivation on it cannot be row-correlated",
      definition.name
    )),
    (Correlation::Of(_), None) => errors.push(format!(
      "{at}: `of` correlates to this object's id, and `{}` has no id column",
      definition.name
    )),
    _ => {}
  }

  // The `where` clause is raw SQL spliced into a subquery, so every column it names must belong to
  // the derivation's own table — that is what lets the compiler retarget it at an alias instead of
  // guessing which table an unqualified name meant.
  if let Some(filter) = &derivation.filter {
    for qualifier in qualifiers_in(filter) {
      if qualifier != derivation.table {
        errors.push(format!(
          "{at}: the `where` clause names `{qualifier}.…`, but it may only qualify with `{}`",
          derivation.table
        ));
      }
    }
  }
}

/// Every `<identifier>.` prefix in a raw SQL fragment.
fn qualifiers_in(sql: &str) -> Vec<String> {
  let bytes = sql.as_bytes();
  let mut out = Vec::new();
  let mut start = None;
  for (index, &byte) in bytes.iter().enumerate() {
    let word = byte.is_ascii_alphanumeric() || byte == b'_';
    match (start, word) {
      (None, true) => start = Some(index),
      (Some(from), false) => {
        if byte == b'.' {
          out.push(sql[from..index].to_owned());
        }
        start = None;
      }
      _ => {}
    }
  }
  out
}

// ── rules 1, 4, 5: rewrite shape ────────────────────────────────────────────────────────────

fn check_rewrite(
  registry: &Registry,
  definition: &Definition,
  rewrite: &Rewrite,
  at: &str,
  depth: usize,
  errors: &mut Vec<String>,
) {
  match rewrite {
    Rewrite::This(name) | Rewrite::Computed(name) => match definition.relation(name) {
      Some(Relation {
        source: RelationSource::Derived(derivation),
        ..
      }) if matches!(derivation.target, EdgeTarget::Object(_)) => errors.push(format!(
        "{at}: `{name}` is an edge to another definition, so it is only usable as \
           `{name}-><permission>`"
      )),
      Some(_) => {}
      None if definition.permission(name).is_some() => {}
      None => errors.push(format!(
        "{at}: `{name}` is neither a relation nor a permission of `{}`",
        definition.name
      )),
    },

    // Rule 4. `^` may only close over a *stored* relation. An exclusion re-evaluated at every
    // ancestor level is not a bounded join, and permitting `^` over a permission is exactly how a
    // `-` would get underneath it.
    Rewrite::Closure(name) => match definition.relation(name).map(|r| &r.source) {
      Some(RelationSource::Stored { .. }) => {}
      Some(RelationSource::Derived(_)) => errors.push(format!(
        "{at}: `{name}^` closes over a derived relation; only a stored relation has ancestors to walk"
      )),
      None if definition.permission(name).is_some() => errors.push(format!(
        "{at}: `{name}^` closes over a permission. Only a stored relation may be closed over — a \
         permission can contain an exclusion, and an exclusion re-evaluated at every ancestor level \
         is not a bounded join"
      )),
      None => {
        errors.push(format!("{at}: `{name}^` names a relation `{}` does not declare", definition.name))
      }
    },

    Rewrite::Walk { edge, next } => {
      if depth + 1 > MAX_WALK_DEPTH {
        errors.push(format!("{at}: `->` chain is deeper than {MAX_WALK_DEPTH}"));
        return;
      }
      let Some(relation) = definition.relation(edge) else {
        errors.push(format!("{at}: `{edge}->…` names a relation `{}` does not declare", definition.name));
        return;
      };
      let RelationSource::Derived(derivation) = &relation.source else {
        errors.push(format!("{at}: `{edge}` is a stored relation, so it cannot be walked through"));
        return;
      };
      let EdgeTarget::Object(target) = &derivation.target else {
        errors.push(format!(
          "{at}: `{edge}` resolves to a subject, not an object, so it cannot be walked through"
        ));
        return;
      };
      let Some(target) = registry.get(target) else { return }; // already reported by rule 6
      check_rewrite(registry, target, next, at, depth + 1, errors);
    }

    Rewrite::Absolute { ty, id, next } => {
      if depth + 1 > MAX_WALK_DEPTH {
        errors.push(format!("{at}: `->` chain is deeper than {MAX_WALK_DEPTH}"));
        return;
      }
      let Some(target) = registry.get(ty) else {
        errors.push(format!("{at}: `{ty}:{id}` names undeclared definition `{ty}`"));
        return;
      };
      if target.table.is_some() {
        errors.push(format!(
          "{at}: `{ty}:{id}` names a row-backed definition; an absolute reference is only for the \
           string-named singletons"
        ));
      }
      check_rewrite(registry, target, next, at, depth + 1, errors);
    }

    Rewrite::Union(parts) | Rewrite::Intersection(parts) => {
      if parts.is_empty() {
        errors.push(format!("{at}: an empty union or intersection"));
      }
      for part in parts {
        check_rewrite(registry, definition, part, at, depth, errors);
      }
    }

    Rewrite::Exclusion { base, subtract } => {
      check_rewrite(registry, definition, base, at, depth, errors);
      check_rewrite(registry, definition, subtract, at, depth, errors);
    }
  }
}

// ── rule 3: cycles ──────────────────────────────────────────────────────────────────────────

/// A depth-first walk of the same graph the compiler inlines, so a cycle it would spin on is a boot
/// failure with the path printed instead of a stack overflow on the first request.
fn check_cycles(registry: &Registry, errors: &mut Vec<String>) {
  let mut done: HashSet<(String, String)> = HashSet::new();
  for definition in registry.definitions() {
    for (permission, _) in &definition.permissions {
      let mut path = Vec::new();
      visit(registry, &definition.name, permission, &mut path, &mut done, errors);
    }
  }
}

fn visit(
  registry: &Registry,
  definition: &str,
  permission: &str,
  path: &mut Vec<(String, String)>,
  done: &mut HashSet<(String, String)>,
  errors: &mut Vec<String>,
) {
  let node = (definition.to_owned(), permission.to_owned());
  if let Some(at) = path.iter().position(|n| *n == node) {
    let cycle = path[at..]
      .iter()
      .map(|(d, p)| format!("{d}#{p}"))
      .chain(std::iter::once(format!("{definition}#{permission}")))
      .collect::<Vec<_>>()
      .join(" → ");
    errors.push(format!("permission cycle: {cycle}"));
    return;
  }
  if done.contains(&node) {
    return;
  }

  let Some(def) = registry.get(definition) else { return };
  let Some(rewrite) = def.permission(permission) else { return };

  path.push(node.clone());
  let mut edges = Vec::new();
  collect_edges(registry, def, rewrite, &mut edges);
  for (next_def, next_perm) in edges {
    visit(registry, &next_def, &next_perm, path, done, errors);
  }
  path.pop();
  done.insert(node);
}

fn collect_edges(
  registry: &Registry,
  definition: &Definition,
  rewrite: &Rewrite,
  out: &mut Vec<(String, String)>,
) {
  match rewrite {
    Rewrite::This(name) | Rewrite::Computed(name) => {
      if definition.relation(name).is_none() && definition.permission(name).is_some() {
        out.push((definition.name.clone(), name.clone()));
      }
    }
    Rewrite::Closure(_) => {}
    Rewrite::Walk { edge, next } => {
      let Some(relation) = definition.relation(edge) else { return };
      let RelationSource::Derived(derivation) = &relation.source else { return };
      let EdgeTarget::Object(target) = &derivation.target else { return };
      let Some(target) = registry.get(target) else { return };
      collect_walk_target(registry, target, next, out);
    }
    Rewrite::Absolute { ty, next, .. } => {
      let Some(target) = registry.get(ty) else { return };
      collect_walk_target(registry, target, next, out);
    }
    Rewrite::Union(parts) | Rewrite::Intersection(parts) => {
      for part in parts {
        collect_edges(registry, definition, part, out);
      }
    }
    Rewrite::Exclusion { base, subtract } => {
      collect_edges(registry, definition, base, out);
      collect_edges(registry, definition, subtract, out);
    }
  }
}

/// The right-hand side of a `->` is evaluated on the target, so a bare name there is one of the
/// target's permissions — which is the edge the cycle check has to follow across definitions.
fn collect_walk_target(
  registry: &Registry,
  target: &Definition,
  next: &Rewrite,
  out: &mut Vec<(String, String)>,
) {
  match next {
    Rewrite::This(name) | Rewrite::Computed(name) if target.permission(name).is_some() => {
      out.push((target.name.clone(), name.clone()));
    }
    other => collect_edges(registry, target, other, out),
  }
}

// ── rule 8: the namespace inventory ─────────────────────────────────────────────────────────

fn check_namespaces(registry: &Registry, errors: &mut Vec<String>) {
  let has_service = registry.get("service").is_some();
  let mut seen: HashSet<&str> = HashSet::new();

  for entry in registry.namespaces() {
    if !seen.insert(entry.namespace.as_str()) {
      errors.push(format!(
        "namespace `{}` is declared twice (second in {})",
        entry.namespace, entry.source
      ));
    }
    if entry.gated && !has_service {
      errors.push(format!(
        "namespace `{}` declares a route gate, but there is no `service` definition to evaluate it",
        entry.namespace
      ));
    }
    if let Some(definition) = &entry.definition
      && registry.get(definition).is_none()
    {
      errors.push(format!(
        "namespace `{}` ({}) resolves to definition `{definition}`, which is not declared — a \
         namespace with no rule is a live resource that is either unreachable or unprotected",
        entry.namespace, entry.source
      ));
    }
  }
}

// ── compilability ───────────────────────────────────────────────────────────────────────────

fn check_compiles(registry: &Registry, errors: &mut Vec<String>) {
  // A subject with one of every shape, so a branch that only appears for a key or only for a user
  // id is still exercised.
  let subject = SubjectSet::from_parts(
    vec!["user:1".to_owned(), "role:1#member".to_owned(), "organization:1#member".to_owned()],
    vec![1],
  );

  for definition in registry.definitions() {
    for (permission, _) in &definition.permissions {
      let compiler = Compiler::new(registry, &subject);
      if definition.table.is_some()
        && let Err(error) = compiler.filter_predicate(&definition.name, permission)
      {
        errors.push(format!("{}#{permission} does not compile to a filter: {error}", definition.name));
      }

      let compiler = Compiler::new(registry, &subject);
      let probe = match &definition.table {
        Some(table) if table.kind.is_int() => ObjectKey::Int(1),
        _ => ObjectKey::Text("probe".to_owned()),
      };
      if let Err(error) = compiler.object_predicate(&definition.name, permission, &probe) {
        errors.push(format!("{}#{permission} does not compile to a check: {error}", definition.name));
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::authz::registry::{RuleFile, SUMIDA_RULES};

  /// Validates one schema in isolation and returns the messages it produced.
  ///
  /// Every case below asserts that a *specific* defect is caught with the offender named. A boot
  /// check nobody has watched fail is a boot check nobody knows works — and this one's whole job is
  /// to turn a silent authorization defect into a process that will not start.
  fn errors_in(text: &'static str) -> Vec<String> {
    let file = RuleFile { name: "test.sz", text };
    match Registry::from_sources(std::slice::from_ref(&file)) {
      Ok(_) => Vec::new(),
      Err(AuthzError::Validation(errors)) => errors,
      Err(other) => vec![other.to_string()],
    }
  }

  fn assert_rejected(text: &'static str, expected: &str) {
    let errors = errors_in(text);
    assert!(
      errors.iter().any(|error| error.contains(expected)),
      "expected a failure mentioning `{expected}`, got {errors:#?}"
    );
  }

  /// Rule 1 — a rewrite naming something that does not exist. In the old model this was a string
  /// that matched no policy, i.e. a silent deny; here it is a boot failure.
  #[test]
  fn a_rewrite_naming_a_missing_relation_is_refused() {
    assert_rejected(
      "definition thing on things.id int { permission view = nonexistent }",
      "`nonexistent` is neither a relation nor a permission",
    );
  }

  /// Rule 2 — a subject type nobody declared. This is the `admin:user` shape: writable, and
  /// matching nothing forever.
  #[test]
  fn a_subject_type_that_does_not_exist_is_refused() {
    assert_rejected(
      "definition thing on things.id int { relation viewer: ghost }",
      "names undeclared definition `ghost`",
    );
  }

  /// Rule 2, E4 — a userset subject that would need recursive resolution. Restricting subjects to
  /// what the session yields in O(1) is what keeps the subject side one string match.
  #[test]
  fn a_userset_subject_that_is_not_session_derivable_is_refused() {
    assert_rejected(
      "definition user { }
       definition team on teams.id int { relation manager: user }
       definition thing on things.id int { relation viewer: team#manager }",
      "not derivable from the session",
    );
  }

  /// Rule 3 — a permission defined in terms of itself. The compiler would inline forever; the path
  /// is printed instead.
  #[test]
  fn a_permission_cycle_is_refused_with_the_path() {
    assert_rejected(
      "definition thing on things.id int {
         permission view = edit
         permission edit = view
       }",
      "permission cycle: thing#view → thing#edit → thing#view",
    );
  }

  /// Rule 4 — `^` over a permission. A permission may contain an exclusion, and an exclusion
  /// re-evaluated at every ancestor level is not a bounded join, so this is where that gets caught.
  #[test]
  fn a_closure_over_a_permission_is_refused() {
    assert_rejected(
      "definition thing on things.id int {
         closure parent via thing_closure
         relation parent: user
         relation blocked: user
         permission own = blocked
         permission view = own^
       }",
      "is not a bounded join",
    );
  }

  /// Rule 4's other half — the closure tables are integer-keyed, so a text-keyed definition cannot
  /// have one. Without this the failure would be a cast error at request time.
  #[test]
  fn a_closure_on_a_text_keyed_definition_is_refused() {
    assert_rejected(
      "definition thing on things.slug {
         closure parent via thing_closure
         relation parent: user
       }",
      "needs integer object ids",
    );
  }

  /// Rule 5 — a `->` chain deeper than four. A plan-quality limit, rejected at boot so nobody meets
  /// it in production.
  #[test]
  fn a_walk_chain_deeper_than_four_is_refused() {
    assert_rejected(
      "definition user { }
       definition f on fs.id int { relation holder: user  permission view = holder }
       definition e on es.id int { relation next: f = column(es.f_id -> f)  permission view = next->view }
       definition d on ds.id int { relation next: e = column(ds.e_id -> e)  permission view = next->view }
       definition c on cs.id int { relation next: d = column(cs.d_id -> d)  permission view = next->view }
       definition b on bs.id int { relation next: c = column(bs.c_id -> c)  permission view = next->view }
       definition a on az.id int  { relation next: b = column(az.b_id -> b)
                                    permission view = next->next->next->next->next->view }",
      "deeper than 4",
    );
  }

  /// Rule 6 — a derivation whose table is not the definition's own and which does not say how it
  /// correlates. Guessing would mean the relation quietly means the opposite of what it says.
  #[test]
  fn a_derivation_on_a_foreign_table_without_of_is_refused() {
    assert_rejected(
      "definition user { }
       definition thing on things.id int { relation owner: user = column(users.id -> user) }",
      "use `of <table>.<column>` to correlate it",
    );
  }

  /// Rule 6 — a `where` clause that reaches outside the derivation's own table. The clause is raw
  /// SQL spliced into a subquery whose table may be aliased, so anything else cannot be retargeted.
  #[test]
  fn a_where_clause_naming_another_table_is_refused() {
    assert_rejected(
      "definition user { }
       definition thing on things.id int {
         relation owner: user = column(things.user_id -> user where users.is_admin)
       }",
      "may only qualify with `things`",
    );
  }

  /// Rule 8 — the check that makes "a namespace left out is a live resource that is unreachable or
  /// unprotected" impossible rather than merely unlikely.
  #[test]
  fn a_namespace_with_no_definition_is_refused() {
    assert_rejected(
      "definition thing on things.id int { permission view = viewer  relation viewer: user }
       definition user { }
       namespace demo:widget = demo_widget",
      "a namespace with no rule is a live resource",
    );
  }

  /// An edge used as if it were a subject relation. The compiler cannot turn an object reference
  /// into a boolean, so this is refused rather than silently dropped.
  #[test]
  fn an_edge_used_as_a_bare_term_is_refused() {
    assert_rejected(
      "definition user { }
       definition other on others.id int { permission view = viewer  relation viewer: user }
       definition thing on things.id int {
         relation link: other = column(things.other_id -> other)
         permission view = link
       }",
      "only usable as `link-><permission>`",
    );
  }

  /// The rule set that actually ships must validate. This is the same check `Registry::global`
  /// performs at boot, asserted here so it cannot regress unnoticed between releases.
  ///
  /// Built from `SUMIDA_RULES` directly rather than through `Registry::global`, so that what it
  /// asserts is sumida's own rules and not whatever a test elsewhere in this binary installed.
  #[test]
  fn the_shipped_rule_set_validates() {
    if let Err(error) = Registry::from_sources(&[SUMIDA_RULES]) {
      panic!("the shipped rule set does not validate:\n{error}");
    }
  }
}
