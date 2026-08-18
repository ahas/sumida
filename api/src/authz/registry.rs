//! The definition registry — the compiled rule set (design §5) and the namespace inventory.
//!
//! The rule text is `include_str!`-ed, so it is baked into the binary and a rule edit is a deploy,
//! not a runtime change (E1). It is parsed once behind a `OnceLock` and validated immediately; a
//! rule set that does not parse or does not validate produces an error every entry point returns,
//! and `Authz::boot` turns that into a refusal to start.
//!
//! **sumida carries its own rules and nobody else's.** A consumer hands its rule files to
//! `install` before anything reads the registry, and the registry is then sumida's file plus
//! whatever was installed. Nothing here enumerates consumers, knows how many there are, or names
//! one — which is the property that lets a product be built on sumida without appearing in it.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::ir::*;
use super::{AuthzError, parse};

/// One rule file. A consumer `include_str!`s its own and hands it over.
#[derive(Clone, Copy, Debug)]
pub struct RuleFile {
  /// The name a parse error is reported against. Use the file name.
  pub name: &'static str,
  pub text: &'static str,
}

/// sumida's own rules. Loaded before anything a consumer installs, and always loaded.
pub const SUMIDA_RULES: RuleFile = RuleFile {
  name: "schema.sz",
  text: include_str!("schema.sz"),
};

/// What a consumer installed, if anything. Read exactly once, by `load`.
static INSTALLED: OnceLock<&'static [RuleFile]> = OnceLock::new();

/// The parsed, validated rule set. `install` refuses once this is set, because from that moment
/// the process has already answered questions with a rule set that would change.
static REGISTRY: OnceLock<Result<Registry, AuthzError>> = OnceLock::new();

#[derive(Debug)]
pub struct Registry {
  definitions: Vec<Definition>,
  by_name:     HashMap<String, usize>,
  namespaces:  Vec<NamespaceEntry>,
  /// Which rule file declared each definition. Report-only: it says which file a boot report is
  /// talking about, and nothing compares it against anything.
  source:      HashMap<String, &'static str>,
}

impl Registry {
  /// Registers a consumer's rule files. Called once, before anything reads the registry.
  ///
  /// Calling it twice with the same slice is fine — building the router and booting each do it, and
  /// they are the same declaration. Calling it with a *different* slice, or after the registry has
  /// been read, is an error: both would mean which rules this process judges by depends on the
  /// order two calls happened to run in, and that is a permission model that differs between
  /// processes reading the same database.
  pub fn install(files: &'static [RuleFile]) -> Result<(), AuthzError> {
    fn same(a: &[RuleFile], b: &[RuleFile]) -> bool {
      a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.name == y.name && x.text == y.text)
    }

    fn names(files: &[RuleFile]) -> String {
      if files.is_empty() {
        "none".to_owned()
      } else {
        files.iter().map(|file| file.name).collect::<Vec<_>>().join(", ")
      }
    }

    if let Some(installed) = INSTALLED.get() {
      if same(installed, files) {
        return Ok(());
      }
      return Err(AuthzError::Schema(format!(
        "rule files are already installed as [{}] and cannot be replaced with [{}]; which rules \
         this process judges by would then depend on the order two calls ran in",
        names(installed),
        names(files)
      )));
    }

    if REGISTRY.get().is_some() {
      // Nothing was installed, so the rule set that was read *is* sumida's alone — which is
      // exactly what an empty slice declares. There is no disagreement to refuse, and refusing
      // would mean a product with no rules of its own could never say so out loud.
      if !files.is_empty() {
        return Err(AuthzError::Schema(format!(
          "[{}] cannot be installed: the rule set has already been read, so this process has \
           already answered questions without them. Install before the first read — above boot \
           validation, not below it",
          names(files)
        )));
      }
    }

    // A concurrent caller may have won the race between the check above and here; the loser is
    // only wrong if it was installing something else.
    match INSTALLED.set(files) {
      Ok(()) => Ok(()),
      Err(_) => Registry::install(files),
    }
  }

  /// Parses and validates sumida's rules plus whatever was installed. Memoised by `global`.
  fn load() -> Result<Self, AuthzError> {
    let installed = INSTALLED.get().copied().unwrap_or(&[]);
    let mut files = Vec::with_capacity(installed.len() + 1);
    files.push(SUMIDA_RULES);
    files.extend_from_slice(installed);
    Self::from_sources(&files)
  }

  /// Parses and validates an arbitrary set of rule files.
  ///
  /// `SUMIDA_RULES` is **not** implied — prepend it when the set is meant to be a whole rule set.
  /// Public so the validation tests can hand it one deliberate defect and assert that the process
  /// would refuse to start, and so a consumer's own golden tests can compile its rules without
  /// touching the process-wide registry. A boot check nobody has watched fail is a boot check
  /// nobody knows works.
  pub fn from_sources(files: &[RuleFile]) -> Result<Self, AuthzError> {
    let mut registry = Registry {
      definitions: Vec::new(),
      by_name:     HashMap::new(),
      namespaces:  Vec::new(),
      source:      HashMap::new(),
    };

    // Extensions are applied after every file is parsed, so a consumer's `extend organization`
    // does not depend on its file being read after the one that declares `organization`.
    let mut extensions: Vec<(&'static str, DefinitionExtension)> = Vec::new();

    for file in files {
      let parsed = parse::parse(file.name, file.text).map_err(AuthzError::Parse)?;
      for definition in parsed.definitions {
        if registry.by_name.contains_key(&definition.name) {
          return Err(AuthzError::Schema(format!(
            "definition `{}` is declared twice; the second is in {}. A definition may be given \
             further members from another file with `extend {}`, but never declared again",
            definition.name, file.name, definition.name
          )));
        }
        registry.source.insert(definition.name.clone(), file.name);
        registry.by_name.insert(definition.name.clone(), registry.definitions.len());
        registry.definitions.push(definition);
      }
      extensions.extend(parsed.extensions.into_iter().map(|extension| (file.name, extension)));
      registry.namespaces.extend(parsed.namespaces);
    }

    for (source, extension) in extensions {
      registry.apply(source, extension)?;
    }

    super::validate::validate(&registry)?;
    Ok(registry)
  }

  /// Adds one extension's members to its target.
  ///
  /// A missing target and a colliding member are both refused. The collision is the one that
  /// matters: a consumer silently redefining sumida's `organization#manage` would be a permission
  /// change nobody wrote, made by adding a file.
  fn apply(&mut self, source: &'static str, extension: DefinitionExtension) -> Result<(), AuthzError> {
    let Some(index) = self.by_name.get(&extension.target).copied() else {
      return Err(AuthzError::Schema(format!(
        "`extend {}` in {source} names a definition no rule file declares",
        extension.target
      )));
    };

    let target = &mut self.definitions[index];
    let added = target.relations.len();

    for relation in extension.relations {
      if target.relation(&relation.name).is_some() {
        return Err(AuthzError::Schema(format!(
          "`extend {}` in {source} declares relation `{}`, which `{}` already has; an extension \
           may only add members, never replace one",
          extension.target, relation.name, extension.target
        )));
      }
      target.relations.push(relation);
    }

    for (name, rewrite) in extension.permissions {
      if target.permission(&name).is_some() {
        return Err(AuthzError::Schema(format!(
          "`extend {}` in {source} declares permission `{name}`, which `{}` already has; an \
           extension may only add members, never replace one",
          extension.target, extension.target
        )));
      }
      target.permissions.push((name, rewrite));
    }

    // The extension was parsed without knowing the target's table, so the correlation fix-up that
    // `definition` does for a table-less definition happens here instead.
    if target.table.is_none() {
      parse::free_correlate(&mut target.relations[added..]);
    }

    Ok(())
  }

  /// The process-wide registry. The error is reproduced on every call, so a boot that ignores it
  /// still cannot get a usable evaluator.
  pub fn global() -> Result<&'static Registry, AuthzError> {
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

  /// The rule file that declared `definition`. Report-only.
  pub fn source_of(&self, definition: &str) -> Option<&'static str> {
    self.source.get(definition).copied()
  }

  pub fn namespace(&self, name: &str) -> Option<&NamespaceEntry> {
    self.namespaces.iter().find(|entry| entry.namespace == name)
  }

  /// Whether `namespace` has a **route gate** for `layer::service_ap` to ask about.
  ///
  /// Two conditions: the namespace is in the inventory — validation rule 8 already guarantees every
  /// namespace that reaches a live call site is, so a `false` here means a namespace nobody
  /// declared — and it declares a route gate, so `service:<namespace>` is a real object.
  pub fn routes_service_gate(&self, namespace: &str) -> bool {
    self.namespace(namespace).is_some_and(|entry| entry.gated)
  }

  /// Every namespace in the inventory that has **no** route gate, with the reason, sorted.
  ///
  /// What it enumerates is the shape that is easy to misread: a namespace whose row tier is checked
  /// directly and which therefore cannot be opened by a `service:<name>` grant, however plausible
  /// one looks written down. The two sets partition the inventory, so a namespace cannot arrive
  /// gateless by accident and be read as gated by a call site.
  pub fn unrouted_namespaces(&self) -> Vec<(&str, &'static str)> {
    let mut out: Vec<(&str, &'static str)> = self
      .namespaces
      .iter()
      .filter(|entry| !entry.gated)
      .map(|entry| (entry.namespace.as_str(), "row-level only, no route gate to route"))
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
  /// A rule is not confined to its own table. `board_file#view` is `post->view | file->view`, so
  /// evaluating it emits subqueries over `board_posts` and `files` — and if one of the columns
  /// those rules read is missing, the statement fails even though every column `board_file`'s
  /// *own* relations read is present. Judging a definition by its own columns alone therefore
  /// reports it backed when it is not, which is the one thing `boot` exists to prevent.
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
mod registry_tests {
  use super::*;

  fn rule(name: &'static str, text: &'static str) -> RuleFile {
    RuleFile { name, text }
  }

  /// Exactly sumida's own gated namespaces route, and nothing else does.
  ///
  /// The list is written out rather than counted. "18 namespaces route" passes just as happily when
  /// one has been swapped for another, and *which* ones is the whole claim — a route gate is what
  /// stands between a caller and an API. What the list also pins is the other direction: the three
  /// namespaces below it that do **not** route, and why.
  #[test]
  fn every_gated_namespace_routes_and_nothing_else_does() {
    let registry = Registry::from_sources(&[SUMIDA_RULES]).expect("the rule set must validate");

    let routed: Vec<&str> = registry
      .namespaces()
      .iter()
      .filter(|entry| registry.routes_service_gate(&entry.namespace))
      .map(|entry| entry.namespace.as_str())
      .collect();

    assert_eq!(
      routed,
      vec![
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
      ]
    );
  }

  /// Every namespace without a route gate is enumerable, with a reason.
  ///
  /// What is asserted is that the two sets **partition** the inventory — no namespace is in
  /// neither list — so one cannot arrive gateless by accident and be read as gated by a call site.
  #[test]
  fn every_unrouted_namespace_is_named_with_a_reason() {
    let registry = Registry::from_sources(&[SUMIDA_RULES]).expect("the rule set must validate");
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

    // The shape that never routes at all: a namespace declared **without** a route gate. Each is a
    // row-level namespace whose object tier is checked directly, and none has a `service:<name>`
    // object for a gate to consult.
    let named: Vec<&str> = unrouted.iter().map(|(name, _)| *name).collect();
    assert_eq!(named, vec!["org:credit", "org:subscription", "org:user"]);
  }

  /// `extend` gives a definition further members from another file, and they behave exactly as if
  /// the definition had declared them.
  #[test]
  fn an_extension_adds_members_to_a_definition_from_another_file() {
    let registry = Registry::from_sources(&[
      SUMIDA_RULES,
      rule("consumer.sz", "extend organization { scope widget }"),
    ])
    .expect("the extension must validate");

    let organization = registry.get("organization").expect("organization is declared");
    for tier in ["widget_viewer", "widget_editor", "widget_manager"] {
      assert!(organization.relation(tier).is_some(), "{tier} must exist");
    }
    for permission in ["view_widget", "edit_widget", "manage_widget"] {
      assert!(organization.permission(permission).is_some(), "{permission} must exist");
    }

    // And the definition is still sumida's — an extension adds, it does not take ownership.
    assert_eq!(registry.source_of("organization"), Some("schema.sz"));
  }

  /// An extension of a definition nobody declares is a boot failure naming the file. Silently
  /// dropping it would mean a consumer's whole resource family quietly does not exist.
  #[test]
  fn an_extension_of_a_missing_definition_is_refused() {
    let error = Registry::from_sources(&[
      SUMIDA_RULES,
      rule("consumer.sz", "extend nonexistent { scope widget }"),
    ])
    .expect_err("must refuse");
    let message = error.to_string();
    assert!(
      message.contains("`extend nonexistent` in consumer.sz"),
      "the failure must name the file and the target: {message}"
    );
  }

  /// A member that collides with one the target already has is a boot failure. This is the case
  /// the construct exists to make impossible: a consumer redefining a framework permission by
  /// adding a file, and nothing saying so.
  #[test]
  fn an_extension_that_collides_with_an_existing_member_is_refused() {
    let error = Registry::from_sources(&[
      SUMIDA_RULES,
      rule("consumer.sz", "extend organization { permission manage = member }"),
    ])
    .expect_err("must refuse");
    let message = error.to_string();
    assert!(
      message.contains("permission `manage`") && message.contains("may only add members"),
      "the failure must name the colliding member: {message}"
    );

    let error = Registry::from_sources(&[
      SUMIDA_RULES,
      rule("consumer.sz", "extend organization { scope user }"),
    ])
    .expect_err("a `scope` that collides is the same failure");
    assert!(error.to_string().contains("user_viewer"), "{error}");
  }

  /// Two files declaring one definition is refused, and the message says what to do instead. This
  /// is the reason `extend` had to be a construct rather than a convention.
  #[test]
  fn a_definition_declared_in_two_files_is_refused() {
    let error = Registry::from_sources(&[
      SUMIDA_RULES,
      rule("consumer.sz", "definition organization on orgs.id int { relation viewer: user }"),
    ])
    .expect_err("must refuse");
    assert!(error.to_string().contains("extend organization"), "{error}");
  }
}
