//! Golden SQL: the exact predicate every rule shape compiles to (design §14 step 2).
//!
//! Pure unit tests — no database, no session, no server. What they assert is the *text* of the
//! compiled predicate, because that text is the whole authorization decision and nothing else in
//! the system shows it. A rewrite that changes meaning changes this text; a refactor that does not
//! changes nothing here.
//!
//! Deliberately literal, aliases included. A golden test that asserts "contains `relation_tuples`"
//! passes on a predicate that grants everyone everything, so the short cases compare whole strings
//! and the long ones pin the load-bearing fragments: the closure's direction, the exclusion's
//! placement, the expiry guard, the shape of the subject match.
//!
//! Alias numbering is deterministic — one counter per compilation, one traversal order — so it
//! moves only when the rule or the compiler moves, which is exactly when a golden test should fail.

use super::subject::SubjectSet;
use super::{Authz, ObjectKey};

/// The caller of design §6.2: user 7, role 3, organization 100.
fn caller() -> SubjectSet {
  SubjectSet::from_parts(
    vec!["user:7".to_owned(), "role:3#member".to_owned(), "organization:100#member".to_owned()],
    vec![7],
  )
}

/// A **secured** API key: one subject, and no user ids at all.
fn secured_key() -> SubjectSet {
  SubjectSet::from_parts(vec!["apikey:deadbeef".to_owned()], vec![])
}

/// The subject match every tuple predicate carries, for the caller above.
const KEYS: &str = r#"ANY(CAST(ARRAY ['user:7','role:3#member','organization:100#member'] AS text[]))"#;

fn sql(definition: &str, permission: &str) -> String {
  Authz::predicate_sql(definition, permission, &caller()).expect("compiles")
}

fn check_sql(definition: &str, id: impl Into<ObjectKey>, permission: &str) -> String {
  Authz::object_predicate_sql(definition, id, permission, &caller()).expect("compiles")
}

// ─── the individual rule shapes ──────────────────────────────────────────────────────────────

/// A stored relation is one `EXISTS` over `relation_tuples`: `resource_type` and `relation` bound
/// as compile-time literals, `resource_id` from the row under test, the subject as one array match,
/// and the expiry guard the old model had no column for.
#[test]
fn stored_relation() {
  assert_eq!(
    check_sql("platform", "root", "superuser").split(" OR EXISTS").next().unwrap(),
    format!(
      r#"EXISTS(SELECT 1 FROM "relation_tuples" AS "_az_t0" WHERE "_az_t0"."resource_type" = 'platform' AND "_az_t0"."relation" = 'admin' AND "_az_t0"."resource_id" = 'root' AND ("_az_t0"."subject_key" = {KEYS}) AND ("_az_t0"."expires_at" IS NULL OR "_az_t0"."expires_at" > (CURRENT_TIMESTAMP)))"#
    )
  );
}

/// A derived relation whose column already holds a canonical subject key: no join, no cast.
/// `tasks.owner_key` is load-bearing for scheduler fairness and is read exactly as it is.
#[test]
fn derived_relation_over_a_subject_key_column() {
  let compiled = sql("joy_task", "view");
  assert!(
    compiled.starts_with(&format!(r#"("tasks"."owner_key" = {KEYS})"#)),
    "the owner branch must be a bare array match on owner_key, and emitted first: {compiled}"
  );
}

/// A derived relation over an integer user column binds the *integer* array, so the comparison is
/// index-usable — and a secured key, whose integer array is empty, cannot match it.
#[test]
fn derived_relation_over_a_user_column() {
  let compiled = sql("board_post", "view");
  assert!(
    compiled.starts_with(
      r#"((("board_posts"."user_id" = ANY(CAST(ARRAY [7] AS int[]))) AND (board_posts.user_id IS NOT NULL))"#
    ),
    "authorship compares integer ids and keeps its `where` clause: {compiled}"
  );
}

/// `platform#flagged` is **uncorrelated**, which is what lets Postgres hoist it into an InitPlan:
/// R1 is evaluated once per statement, not once per row.
#[test]
fn r1_is_an_uncorrelated_exists() {
  let compiled = check_sql("platform", "root", "superuser");
  assert!(
    compiled.ends_with(
      r#"EXISTS(SELECT 1 FROM "users" AS "_az_m1" WHERE ("_az_m1"."id" = ANY(CAST(ARRAY [7] AS int[]))) AND ("_az_m1".is_admin))"#
    ),
    "the is_admin branch must not correlate to the outer row: {compiled}"
  );
}

/// The same rule judged as a **secured API key** binds an empty integer array, so the
/// `users.is_admin` branch is statically false. This is the structural fix for "a secured key bound
/// to an admin keeps full administrative power" — and the key can still hold an explicit grant, so
/// the fix narrows without breaking the feature.
#[test]
fn a_secured_key_cannot_reach_the_is_admin_branch() {
  let compiled =
    Authz::object_predicate_sql("platform", "root", "superuser", &secured_key()).expect("compiles");
  assert!(
    compiled.contains(r#""_az_m1"."id" = ANY(CAST('{}' AS int[]))"#),
    "a secured key must bind no user ids at all: {compiled}"
  );
  assert!(
    compiled.contains(r#""_az_t0"."subject_key" = ANY(CAST(ARRAY ['apikey:deadbeef'] AS text[]))"#),
    "and must still be able to hold an explicit platform grant: {compiled}"
  );
}

/// `^` walks the closure **from the resource upward**: `descendant_id` is the resource's
/// organization, `ancestor_id` is where the grant sits. Inverted, R2 would reach ancestors instead
/// of descendants — the single most consequential thing in this file to get backwards. The
/// subquery is uncorrelated, so Postgres hoists it into an InitPlan.
#[test]
fn closure_walks_from_the_resource_up_to_the_grant() {
  assert_eq!(
    check_sql("organization", 100, "manage"),
    format!(
      r#"100 IN (SELECT "_az_c1"."descendant_id" FROM "relation_tuples" AS "_az_m0" INNER JOIN "org_closure" AS "_az_c1" ON "_az_c1"."ancestor_id" = "_az_m0"."resource_id_int" WHERE "_az_m0"."resource_type" = 'organization' AND "_az_m0"."relation" = 'manager' AND ("_az_m0"."subject_key" = {KEYS}) AND ("_az_m0"."expires_at" IS NULL OR "_az_m0"."expires_at" > (CURRENT_TIMESTAMP)))"#
    )
  );
}

/// Four closure branches of the same shape collapse into one `relation IN (…)`, so R2 costs one
/// index probe rather than four. This is `scope task`'s three tiers plus the blanket `manager`,
/// which is what makes `organization:100#manager@user:9` the whole of R2 for that person.
#[test]
fn same_shape_closure_branches_are_flattened() {
  let compiled = check_sql("organization", 100, "view_task");
  assert!(
    compiled.contains(r#""relation" IN ('task_viewer', 'task_editor', 'task_manager', 'manager')"#),
    "the three tiers and the blanket manager must be one probe: {compiled}"
  );
  assert_eq!(compiled.matches("SELECT").count(), 1, "and exactly one subquery: {compiled}");
}

/// Stored branches of the same shape collapse too — `can_read = read | operate_read` with
/// `operate_read = op_read | operator | platform:root->superuser`.
#[test]
fn same_shape_stored_branches_are_flattened() {
  let compiled = check_sql("service", "trainer:task", "can_read");
  assert!(
    compiled.contains(
      r#""_az_t0"."relation" IN ('read', 'op_read', 'operator') AND "_az_t0"."resource_id" = 'trainer:task'"#
    ),
    "the route gate and the two object-wide grants are one probe, and the service object id keeps \
     today's colon form: {compiled}"
  );
}

/// A wildcard-admitting relation gets the `user:*` branch; one that never declared a wildcard does
/// not — so a stray wildcard tuple cannot take effect where nobody allowed one.
#[test]
fn the_public_wildcard_is_added_only_where_declared() {
  let public = sql("board", "view");
  assert!(
    public
      .contains(&format!(r#"(("_az_t0"."subject_key" = {KEYS}) OR "_az_t0"."subject_key" = 'user:*')"#)),
    "board#viewer declares `user:*`, so a public board is one tuple: {public}"
  );

  let private = sql("board_group", "view");
  assert!(
    !private.contains("'user:*'"),
    "board_group declares no wildcard, so no wildcard branch may appear: {private}"
  );
}

/// `->` over a derived edge substitutes the target's id expression, with no join at all when the
/// target's own rules do not read its table.
#[test]
fn walk_over_an_edge_substitutes_the_target_id() {
  let compiled = sql("joy_task", "view");
  assert!(
    compiled.contains(
      r#""_az_t1"."resource_type" = 'delegation_scope' AND "_az_t1"."relation" = 'operator' AND "_az_t1"."resource_id" = "tasks"."owner_scope""#
    ),
    "the delegation branch compares the tuple's resource_id to the task's scope column, with no \
     join: {compiled}"
  );
  assert!(
    compiled.contains("AND (tasks.owner_scope IS NOT NULL)"),
    "and the edge's `where` clause gates the whole walk, so a task with no scope is not delegated: \
     {compiled}"
  );
}

/// A multi-hop derivation (E5) becomes nested `EXISTS`, one per join — the same two joins
/// `lab-app:site`'s inspection path pays for today, and no more.
#[test]
fn multi_hop_derivation_becomes_nested_exists() {
  let compiled = sql("daqs_inspection", "view");
  assert!(
    compiled.starts_with(
      r#"EXISTS(SELECT 1 FROM "photos" AS "_az_h0" WHERE "_az_h0"."id" = "inspections"."photo_id" AND EXISTS(SELECT 1 FROM "structures" AS "_az_h1" WHERE "_az_h1"."id" = "_az_h0"."structure_id" AND EXISTS(SELECT 1 FROM "sites" AS "_az_o2" WHERE "_az_o2"."id" = "_az_h1"."site_id""#
    ),
    "inspections → photos → structures → sites, in that order: {compiled}"
  );
}

/// A walk into a definition whose own rules read its table is wrapped in an `EXISTS` over that
/// table. Without the wrap those columns would have no `FROM` and the statement would not run.
#[test]
fn walk_into_a_row_reading_definition_is_wrapped() {
  let compiled = sql("ban", "view");
  assert!(
    compiled.contains(r#"EXISTS(SELECT 1 FROM "users" AS "_az_o0" WHERE "_az_o0"."id" = "bans"."user_id""#),
    "the target row is fetched so `sumida_user`'s own columns resolve: {compiled}"
  );
  assert!(
    compiled.contains(r#""_az_t1"."resource_id" = CAST("_az_o0"."id" AS text)"#),
    "and the wrap's alias, not the real table name, is what the inner predicate reads: {compiled}"
  );
}

/// The userset direction. `organization#member` is "the users whose organization is this object",
/// which is the opposite of every other derivation in the rule set and the reason `of` exists.
#[test]
fn a_userset_relation_scans_in_the_other_direction() {
  let compiled = check_sql("organization", 100, "view");
  assert!(
    compiled.starts_with(
      r#"EXISTS(SELECT 1 FROM "users" AS "_az_m0" WHERE "_az_m0"."organization_id" = 100 AND ("_az_m0"."id" = ANY(CAST(ARRAY [7] AS int[]))))"#
    ),
    "membership correlates the *table* to the object, not the other way round: {compiled}"
  );
}

/// Intersection is preserved rather than converted to a union — `board_group AND board` is what
/// `ApChain` composes today and what `board_post`'s create path actually checks.
#[test]
fn intersection_survives_as_and() {
  let compiled = sql("board", "edit");
  assert!(
    compiled.contains(
      r#""_az_m7"."resource_type" = 'board_group' AND "_az_m7"."relation" IN ('editor', 'owner')"#
    ),
    "the group half must be there: {compiled}"
  );
  assert!(
    compiled.contains(
      r#"AND (boards.group_id IS NOT NULL) AND (EXISTS(SELECT 1 FROM "relation_tuples" AS "_az_t14" WHERE "_az_t14"."resource_type" = 'board' AND "_az_t14"."relation" = 'editor'"#
    ),
    "and it must compose with an AND against the board's own half, not fold into the union: \
     {compiled}"
  );
}

/// Exclusion binds **outside the whole union**, so authorship cannot route around a block. This is
/// `authorize_post_mutation`'s "a deny is absolute and cannot be circumvented via ownership",
/// expressed once in the rule instead of at every call site.
#[test]
fn exclusion_subtracts_a_union_from_the_whole_union() {
  let compiled = sql("board_post", "edit");

  // The subtraction that belongs to `board_post#edit` itself is the one at bracket depth zero;
  // the deeper ones belong to the `board` and `board_group` rules it walks into.
  let mut depth = 0usize;
  let mut top = None;
  for (index, byte) in compiled.bytes().enumerate() {
    match byte {
      b'(' => depth += 1,
      b')' => depth -= 1,
      _ => {}
    }
    if depth == 0 && compiled[index..].starts_with("AND (NOT ") {
      top = Some(index);
      break;
    }
  }
  let cut = top.expect("board_post#edit must subtract its block at the top level");
  let (allowed, blocked) = compiled.split_at(cut);

  assert!(
    allowed.contains(r#""board_posts"."user_id" = ANY"#),
    "authorship is inside the allowed half: {allowed}"
  );
  assert!(
    blocked.starts_with(
      r#"AND (NOT (EXISTS(SELECT 1 FROM "relation_tuples" AS "_az_t24" WHERE "_az_t24"."resource_type" = 'board_post' AND "_az_t24"."relation" = 'blocked'"#
    ),
    "and the block is subtracted from all of it: {blocked}"
  );
  assert!(
    blocked.contains(r#""resource_type" = 'board'"#) && blocked.contains("'board_group'"),
    "the subtrahend is a union of the post's, the board's and the group's blocks: {blocked}"
  );
}

/// `a - b - c` is `(a - b) - c`, which is `a - (b | c)`: the subtraction stays outside the union it
/// applies to, however many times it appears.
#[test]
fn a_chained_exclusion_is_one_subtraction_of_a_union() {
  let compiled = sql("board_group", "view");
  assert_eq!(
    compiled.matches("AND (NOT ").count(),
    2,
    "`view` subtracts its own block, and so does the `edit` it unions: {compiled}"
  );
}

// ─── the rules of §5, end to end ─────────────────────────────────────────────────────────────

/// §6.2's worked example. Every branch it names must be present, and the whole thing must still be
/// one expression — no CTE, no second statement, nothing to feed back into SQL. That is the
/// property trainer's 15 000-row task list and lab-app's 13 647 photos depend on.
#[test]
fn joy_task_view_matches_the_worked_example() {
  let compiled = sql("joy_task", "view");

  for (branch, fragment) in [
    ("R3 owner", r#""tasks"."owner_key" = ANY"#),
    ("per-object grant", r#""_az_t0"."relation" IN ('viewer', 'operator')"#),
    ("delegated owner", r#""resource_type" = 'delegation_scope'"#),
    ("R2 creator org", r#""org_closure""#),
    ("service operator", r#""_az_t7"."resource_id" = 'trainer:task'"#),
    ("R1 flag", ".is_admin"),
    ("R1 grant", r#""resource_type" = 'platform'"#),
  ] {
    assert!(compiled.contains(fragment), "{branch} is missing from joy_task#view: {compiled}");
  }

  assert!(!compiled.contains("WITH "), "no CTE — the rule must be one predicate: {compiled}");
  assert!(!compiled.contains(';'), "one statement only: {compiled}");
}

/// R2 reaches the **creator's** organization and every ancestor of it, through the user who created
/// the task. A grant on an ancestor reaches the descendant, never the reverse.
#[test]
fn joy_task_reaches_the_creators_org_and_its_ancestors() {
  let compiled = sql("joy_task", "view");
  assert!(
    compiled.contains(
      r#"EXISTS(SELECT 1 FROM "users" AS "_az_h4" WHERE "_az_h4"."id" = "tasks"."owner_user_id" AND "_az_h4"."organization_id" IN (SELECT "_az_c6"."descendant_id""#
    ),
    "the organization is the creator's, and the closure is walked from it: {compiled}"
  );
  assert!(
    compiled
      .contains(r#""_az_m5"."relation" IN ('task_viewer', 'task_editor', 'task_manager', 'manager')"#),
    "with all four organization tiers in one probe: {compiled}"
  );
}

/// site-app's project scope. The `^` closure replaces `organization_ancestors_subquery`'s per-row
/// recursive CTE, and read/update/owner become viewer/editor/manager on the organization.
#[test]
fn antares_project_view_replaces_the_ancestor_subquery() {
  let compiled = sql("antares_project", "view");

  assert!(
    compiled.contains(r#""projects"."organization_id" IN (SELECT "#)
      && compiled.contains(r#""org_closure""#),
    "the project's own organization enters the closure: {compiled}"
  );
  assert!(
    compiled
      .contains(r#""relation" IN ('project_viewer', 'project_editor', 'project_manager', 'manager')"#),
    "read maps to the viewer tier, which the editor and manager tiers imply: {compiled}"
  );
  assert!(
    compiled.starts_with(&format!(
      r#"EXISTS(SELECT 1 FROM "relation_tuples" AS "_az_t0" WHERE "_az_t0"."resource_type" = 'antares_project' AND "_az_t0"."relation" IN ('owner', 'editor', 'viewer') AND "_az_t0"."resource_id" = "projects"."slug" AND ("_az_t0"."subject_key" = {KEYS})"#
    )),
    "the per-slug grant is one probe over the three levels: {compiled}"
  );
  assert!(
    compiled.contains(
      r#"("projects"."owner_user_id" = ANY(CAST(ARRAY [7] AS int[]))) AND (projects.owner_user_id IS NOT NULL)"#
    ),
    "and the owner column stays an owner: {compiled}"
  );
  assert!(
    !compiled.contains("RECURSIVE"),
    "the per-row recursive CTE is exactly what this replaces: {compiled}"
  );
}

/// lab-app's site rule: no organization branch at all, and one block subtracted from everything.
/// Gaining an ancestor walk here would silently widen production, which is why its absence is
/// asserted rather than assumed.
#[test]
fn daqs_site_has_no_org_branch_and_one_block() {
  let compiled = sql("daqs_site", "view");
  assert!(!compiled.contains("org_closure"), "lab-app has no hierarchy on sites today: {compiled}");
  assert!(
    compiled.contains(
      r#"("sites"."manager_id" = ANY(CAST(ARRAY [7] AS int[]))) AND (sites.manager_id IS NOT NULL)"#
    ),
    "the manager column is the ownership edge: {compiled}"
  );
  assert!(
    compiled.ends_with(&format!(
      r#"AND (NOT EXISTS(SELECT 1 FROM "relation_tuples" AS "_az_t9" WHERE "_az_t9"."resource_type" = 'daqs_site' AND "_az_t9"."relation" = 'blocked' AND "_az_t9"."resource_id" = CAST("sites"."id" AS text) AND ("_az_t9"."subject_key" = {KEYS}) AND ("_az_t9"."expires_at" IS NULL OR "_az_t9"."expires_at" > (CURRENT_TIMESTAMP))))"#
    )),
    "and the block is subtracted last, outside everything: {compiled}"
  );
}

/// Every rule in the set compiles, in both directions. This is the "a rule that cannot compile must
/// stop start-up" requirement, asserted rather than assumed.
#[test]
fn every_permission_in_the_rule_set_compiles() {
  let registry = Authz::registry().expect("the rule set must validate");
  for definition in registry.definitions() {
    for (permission, _) in &definition.permissions {
      if definition.table.is_some() {
        Authz::predicate_sql(&definition.name, permission, &caller())
          .unwrap_or_else(|e| panic!("{}#{permission} does not compile to a filter: {e}", definition.name));
      }
      let probe = match &definition.table {
        Some(table) if table.kind.is_int() => ObjectKey::Int(1),
        _ => ObjectKey::Text("probe".to_owned()),
      };
      Authz::object_predicate_sql(&definition.name, probe, permission, &caller())
        .unwrap_or_else(|e| panic!("{}#{permission} does not compile to a check: {e}", definition.name));
    }
  }
}

// ─── the inventory ───────────────────────────────────────────────────────────────────────────

/// Every namespace found by grepping `service_ap!` and `ap::ns(` in the four products is declared,
/// and the counts match that enumeration. A count is a weak assertion on its own; it is here so
/// that *adding* a namespace to the code without adding a rule fails loudly rather than quietly.
#[test]
fn the_namespace_inventory_matches_the_code() {
  let registry = Authz::registry().expect("validates");
  let namespaces = registry.namespaces();

  let gated = namespaces.iter().filter(|n| n.gated).count();
  assert_eq!(gated, 44, "44 service objects, one per route-gated namespace");

  let per_product = |product: &str| namespaces.iter().filter(|n| n.product == product).count();
  assert_eq!(per_product("sumida"), 21, "18 gates + org:user, org:credit, org:subscription");
  assert_eq!(per_product("trainer"), 9, "8 gates + trainer:delegate");
  assert_eq!(per_product("site-app"), 11, "10 gates + site-app:project.row");
  assert_eq!(per_product("lab-app"), 8, "7 gates + the unprefixed downloadable_asset");

  for entry in namespaces {
    if let Some(definition) = &entry.definition {
      assert!(
        registry.get(definition).is_some(),
        "namespace `{}` resolves to `{definition}`, which is not declared",
        entry.namespace
      );
    }
  }
}

/// The three namespaces site-app's initiator seeds and nothing gates on stay out. Manufacturing a
/// `service:` object for a typo is how a dead grant becomes a live one.
#[test]
fn the_known_orphan_namespaces_are_not_declared() {
  let registry = Authz::registry().expect("validates");
  for orphan in ["user_log", "user_block", "site-app:node"] {
    assert!(
      !registry.namespaces().iter().any(|n| n.namespace == orphan),
      "`{orphan}` is gated on by nothing and must not gain a rule"
    );
  }
}

/// `org:credit` and `org:subscription` keep a `use` tier distinct from `manage` — blocking item 2
/// of the brief. `credit/mod.rs:66` uses `manage` and `:93` uses `use`, so collapsing them would
/// remove a capability that is in use.
#[test]
fn credit_and_subscription_keep_a_use_tier_distinct_from_manage() {
  let registry = Authz::registry().expect("validates");
  let organization = registry.get("organization").expect("organization is declared");

  for family in ["credit", "subscription"] {
    for tier in ["view", "edit", "manage"] {
      assert!(
        organization.permission(&format!("{tier}_{family}")).is_some(),
        "organization#{tier}_{family} must exist"
      );
    }
  }

  // The two tiers must not compile to the same predicate, or the distinction is decorative.
  let using = check_sql("organization", 100, "view_credit");
  let managing = check_sql("organization", 100, "manage_credit");
  assert_ne!(using, managing, "`use` and `manage` must be different grants");
  assert!(
    using.contains("'credit_viewer'") && !managing.contains("'credit_viewer'"),
    "the viewer tier reaches `use` and not `manage`"
  );
}

/// `CreditService::authorize_use` short-circuits on "same-organization member"
/// (`credit/mod.rs:88`). That is `organization#member`, and `view_credit` has to keep it or a
/// member loses the ability to spend their own organization's credit.
#[test]
fn view_credit_still_admits_a_plain_organization_member() {
  let compiled = check_sql("organization", 100, "view_credit");
  assert!(
    compiled.contains(
      r#"WHERE "_az_m2"."organization_id" = 100 AND ("_az_m2"."id" = ANY(CAST(ARRAY [7] AS int[])))"#
    ),
    "membership must remain a branch of view_credit: {compiled}"
  );
}
