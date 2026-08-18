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

/// The products whose namespaces are **routed to the compiled evaluator** (design §11, Phase 1).
///
/// Migration is per-namespace opt-in, not a global flip: a namespace nobody has opted in for
/// behaves exactly as it does today, because it *is* today's code path. This list is the opt-in,
/// and each consumer appends its own name in its own Phase 2 — trainer at §14 step 8, site-app at step
/// 9, lab-app at step 10 (lab-app behind a per-namespace list of its own, so `lab-app:site` can be switched
/// last and switched back).
///
/// It is a `const` rather than configuration on purpose. A routing decision that can change at run
/// time is a permission model that differs between two processes reading the same database, and
/// the only way to find out which one a request got is to have been watching.
///
/// trainer joined at §14 step 8, once `src/migration/joy_service_init.rs` grew the five columns its
/// rules read. That order is the invariant, not a courtesy: until the columns existed `Authz::boot`
/// reported trainer's definitions as drift and `boot_or_abort` refused to start, so opting in earlier
/// would have routed a gate whose row tier could not be evaluated.
///
/// site-app joined at §14 step 9. It needed no columns — `antares_acl_init.rs` had already added
/// `projects.owner_user_id` and `projects.organization_id` for the legacy row scope, and the rule
/// reads the same two. What it needed instead was for its own writers to stop writing
/// `access_policies` rows directly (`project/mod.rs`, `project/shares.rs`), because a product that
/// reads tuples while writing policy rows depends on the mirror trigger in the one direction the
/// mirror does not run.
///
/// **Since §14 step 14 there is nothing to opt out of.** `util/ap.rs` is deleted, so a namespace
/// that is not routed does not fall back — it simply has no route gate to ask about, and
/// `unrouted_namespaces` now enumerates exactly the namespaces declared *without* one. The list is
/// kept because it is still the record of which product declared what, and because the two tests
/// below read the inventory through it; it is no longer a switch, and adding a fifth product to it
/// grants nothing on its own.
///
/// lab-app joined at §14 step 13, and with it the last consumer of the legacy evaluator went — which
/// is the point of routing it at all. Its rules were already written (`schema.lab-app.zed`); what it
/// needed was three things. Its `Migrator` had to register `rebac_columns`, because
/// `boards.organization_id` is read by the `board` definition that `daqs_inquiry` and
/// `daqs_inquiry_rating` walk to. Its own writers had to stop writing `access_policies` rows — the
/// site-sharing grid (`site/mod.rs`) is one of the five writers the mirror was built for, and a
/// product that reads tuples while writing policy rows is reading a copy that is only as fresh as a
/// trigger. And its one raw-SQL *reader* of the policy table (the organization filter on the site
/// list) had to be re-asked of `relation_tuples`.
pub const ROUTED_PRODUCTS: &[&str] = &["sumida", "trainer", "site-app", "lab-app"];

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

  pub fn namespace(&self, name: &str) -> Option<&NamespaceEntry> {
    self.namespaces.iter().find(|entry| entry.namespace == name)
  }

  /// Whether `namespace` has a **route gate** for `layer::service_ap` to ask about.
  ///
  /// Three conditions, all necessary:
  ///
  /// * the namespace is in the inventory — validation rule 8 already guarantees every namespace
  ///   that reaches a live call site is, so a `false` here means a namespace nobody declared;
  /// * it declares a route gate, so `service:<namespace>` is a real object;
  /// * its product is in `ROUTED_PRODUCTS`, which all four now are.
  ///
  /// A `false` used to mean "answered by the legacy evaluator instead"; with that evaluator gone it
  /// means "there is no gate here", and the only namespaces it is false for are the four declared
  /// without one.
  pub fn routes_service_gate(&self, namespace: &str) -> bool {
    self
      .namespace(namespace)
      .is_some_and(|entry| entry.gated && ROUTED_PRODUCTS.contains(&entry.product.as_str()))
  }

  /// Every namespace in the inventory that has **no** route gate, with the reason, sorted.
  ///
  /// It was written when the alternative was a silent fallback to `access_policies` — a resource
  /// nobody could enumerate is a resource nobody knows is unmigrated. There is no fallback now, and
  /// what it enumerates instead is the shape that is easy to misread: a namespace whose row tier is
  /// checked directly and which therefore cannot be opened by a `service:<name>` grant, however
  /// plausible one looks written down.
  pub fn unrouted_namespaces(&self) -> Vec<(&str, &'static str)> {
    let mut out: Vec<(&str, &'static str)> = self
      .namespaces
      .iter()
      .filter_map(|entry| {
        match (entry.gated, ROUTED_PRODUCTS.contains(&entry.product.as_str())) {
          (true, true) => None,
          (false, _) => Some((entry.namespace.as_str(), "row-level only, no route gate to route")),
          (true, false) => Some((entry.namespace.as_str(), "product has not opted in")),
        }
      })
      .collect();
    out.sort_unstable();
    out
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

  /// `name` and every definition its permissions can reach, transitively.
  ///
  /// A rule is not confined to its own table. `joy_terminal_session#view` is
  /// `owner | agent->view | …`, so evaluating it emits a subquery over `agents` — and if
  /// `agents.organization_id` is missing, the statement fails even though every column
  /// `joy_terminal_session`'s *own* relations read is present. Judging a definition by its own
  /// columns alone therefore reports it backed when it is not, which is the one thing `boot` exists
  /// to prevent.
  ///
  /// Only `->` and `type:id->` leave the definition; a computed permission stays on it and is
  /// already covered by the definition's own relations.
  pub fn reachable_from(&self, name: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut queue: Vec<&str> = vec![name];

    while let Some(current) = queue.pop() {
      let Some(definition) = self.get(current) else { continue };
      if out.contains(&definition.name.as_str()) {
        continue;
      }
      out.push(&definition.name);

      for (_, rewrite) in &definition.permissions {
        self.collect_targets(definition, rewrite, &mut queue);
      }
    }

    out.sort_unstable();
    out
  }

  /// Every definition `rewrite` steps out to, as names.
  ///
  /// A walk's right-hand side is judged against the **target**, not against the definition the walk
  /// started from, so a chain (`board->group->blocked`) has to change scope at each hop or the
  /// second edge resolves against the wrong relation set.
  fn collect_targets<'a>(
    &'a self,
    definition: &'a Definition,
    rewrite: &'a Rewrite,
    out: &mut Vec<&'a str>,
  ) {
    match rewrite {
      Rewrite::Walk { edge, next } => {
        let Some(Relation {
          source: RelationSource::Derived(derivation),
          ..
        }) = definition.relation(edge)
        else {
          return;
        };
        let EdgeTarget::Object(target) = &derivation.target else { return };
        out.push(target);
        if let Some(target_definition) = self.get(target) {
          self.collect_targets(target_definition, next, out);
        }
      }
      Rewrite::Absolute { ty, next, .. } => {
        out.push(ty);
        if let Some(target_definition) = self.get(ty) {
          self.collect_targets(target_definition, next, out);
        }
      }
      Rewrite::Union(parts) | Rewrite::Intersection(parts) => {
        for part in parts {
          self.collect_targets(definition, part, out);
        }
      }
      Rewrite::Exclusion { base, subtract } => {
        self.collect_targets(definition, base, out);
        self.collect_targets(definition, subtract, out);
      }
      Rewrite::This(_) | Rewrite::Computed(_) | Rewrite::Closure(_) => {}
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDependency {
  pub definition: String,
  pub table:      String,
  pub column:     String,
}

#[cfg(test)]
mod routing_tests {
  use super::*;

  /// Exactly the four products' gated namespaces route, and nothing else does.
  ///
  /// The list is written out rather than counted. "44 namespaces route" passes just as happily
  /// when one of lab-app's has been swapped in for one of site-app's, and the whole claim of the phased
  /// migration is *which* ones — a consumer that has not migrated must be on the code path it is on
  /// today, byte for byte. With lab-app opted in at §14 step 13 all four are here, and what the list
  /// now pins is the other direction: the five namespaces below it that still do **not** route, and
  /// why.
  #[test]
  fn only_the_opted_in_products_route_gates_are_routed() {
    let registry = Registry::load().expect("the rule set must validate");

    let routed: Vec<&str> = registry
      .namespaces()
      .iter()
      .filter(|entry| registry.routes_service_gate(&entry.namespace))
      .map(|entry| entry.namespace.as_str())
      .collect();

    assert_eq!(
      routed,
      vec![
        // sumida — §14 step 7
        "access_log",
        "access_policy",
        "api_key",
        "ban",
        "board",
        "board_file",
        "board_group",
        "board_post",
        "board_post_reaction",
        "board_post_report",
        "board_topic",
        "file",
        "mail_form",
        "membership_plan",
        "organization",
        "role",
        "user",
        "user_invitation",
        // trainer — §14 step 8. `trainer:delegate` is absent on purpose: it has no route gate at all, so
        // there is nothing to route; its object tier is `delegation_scope`, checked directly.
        "trainer:agent",
        "trainer:cloud",
        "trainer:cluster",
        "trainer:dashboard",
        "trainer:job",
        "trainer:system-settings",
        "trainer:task",
        "trainer:terminal-session",
        // site-app — §14 step 9. `site-app:project.row` is absent on purpose, for the same reason
        // `trainer:delegate` is: it is the row namespace and has no route gate at all. site-app uses a
        // *different* namespace for the gate deliberately, so a project whose slug is "service"
        // cannot collide with the role gate seed.
        "site-app:artifact",
        "site-app:cache",
        "site-app:chunk",
        "site-app:gcp",
        "site-app:image",
        "site-app:job",
        "site-app:project",
        "site-app:survey",
        "site-app:system-settings",
        "site-app:workflow",
        // lab-app — §14 step 13. `downloadable_asset` is here without a `lab-app:` prefix because
        // lab-app's row-level check used the unprefixed name while its route gate used the prefixed
        // one; one definition honours both, so neither existing grant is lost.
        "lab-app:ai_model",
        "lab-app:downloadable_asset",
        "lab-app:inspection",
        "lab-app:photo",
        "lab-app:report",
        "lab-app:site",
        "lab-app:structure",
        "downloadable_asset",
      ]
    );
  }

  /// Every namespace without a route gate is enumerable, with a reason.
  ///
  /// What is asserted is that the two sets **partition** the inventory — no namespace is in neither
  /// list. That was the "never silent" half while a fallback existed; it is now the check that
  /// every declared namespace is classified, so one cannot arrive gateless by accident and be read
  /// as gated by a call site.
  #[test]
  fn every_unrouted_namespace_is_named_with_a_reason() {
    let registry = Registry::load().expect("the rule set must validate");
    let unrouted = registry.unrouted_namespaces();

    for (namespace, reason) in &unrouted {
      assert!(!reason.is_empty(), "`{namespace}` is unrouted with no reason given");
      assert!(
        !registry.routes_service_gate(namespace),
        "`{namespace}` is listed as unrouted and routes anyway"
      );
    }

    let routed = registry.namespaces().iter().filter(|e| registry.routes_service_gate(&e.namespace));
    assert_eq!(
      unrouted.len() + routed.count(),
      registry.namespaces().len(),
      "a namespace is in neither list, so nothing here says whether it has a route gate"
    );

    // All four products have opted in, so no namespace is unrouted for that reason any more. What
    // is left is the shape that never routes at all: a namespace declared **without** a route gate,
    // one per product that has one. Each is a row-level namespace whose object tier is checked
    // directly, and none of them has a `service:<name>` object for a gate to consult.
    for namespace in ["org:user", "org:credit", "trainer:delegate", "site-app:project.row"] {
      assert!(
        unrouted.iter().any(|(name, _)| *name == namespace),
        "`{namespace}` is no longer reported as unrouted"
      );
    }
  }
}
