use sea_orm::{ConnectionTrait, DbErr, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// Backfill of `access_policies` into `relation_tuples`, plus the shadow comparator that says
/// where the two stores disagree (design §9.1, §9.2).
///
/// Still nothing *reads* the tuples. This migration only makes the second store hold the same
/// grants as the first, so that the comparator can be run against a real database and the
/// difference argued about before any evaluator exists.
///
/// The one thing this file is strict about: **a policy row is either translated or reported.**
/// There is no third outcome. A row the mapping does not cover aborts the migration with the row
/// printed, because the failure mode of guessing is a permission that quietly disappears or
/// quietly widens, and neither shows up until someone is locked out or reads something they
/// should not.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Namespaces that get a `service` object (design §5.1). 44 names, produced by enumerating every
/// `service_ap!` route gate and every row-level `ap::ns` in sumida, trainer, site-app and lab-app.
///
/// This list is duplicated from the design on purpose and it is temporary: the rule registry of a
/// later phase owns the inventory, and validation rule 8 turns a missing namespace into a boot
/// failure. Until that exists, the backfill has no other way to tell "a namespace somebody
/// declared" from "a typo that has been sitting in the table for a year", and inventing a
/// `service:` object for the second is exactly the silent migration this phase must not do.
pub const SERVICE_NAMESPACES: &[&str] = &[
  // sumida — 18
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
  // trainer — 8
  "trainer:agent",
  "trainer:cloud",
  "trainer:cluster",
  "trainer:dashboard",
  "trainer:job",
  "trainer:system-settings",
  "trainer:task",
  "trainer:terminal-session",
  // site-app — 10
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
  // lab-app — 8. `downloadable_asset` has no route gate of its own; it is here because lab-app's
  // row-level checks use the unprefixed name and existing grants have to keep matching.
  "lab-app:ai_model",
  "lab-app:downloadable_asset",
  "lab-app:inspection",
  "lab-app:photo",
  "lab-app:report",
  "lab-app:site",
  "lab-app:structure",
  "downloadable_asset",
];

/// Namespaces seeded by `site-app/src/bin/initiator.rs` that nothing anywhere gates on. Two of
/// them do not even exist as sumida concepts. Their rows are dropped, with a line each.
pub const ORPHAN_NAMESPACES: &[&str] = &["user_log", "user_block", "site-app:node"];

/// One `access_policies` row and what the backfill decided to do with it.
#[derive(Debug, FromQueryResult)]
pub struct PolicyDisposition {
  pub namespace:   String,
  pub object:      String,
  pub action:      String,
  pub subject:     String,
  pub effect:      String,
  /// `migrate:<target>` · `drop:<reason>` · `refuse:<reason>`.
  pub disposition: String,
}

impl PolicyDisposition {
  fn render(&self) -> String {
    format!(
      "{:<22} | {:<12} | {{{}}} | {:<12} | {:<5} → {}",
      self.namespace, self.object, self.action, self.subject, self.effect, self.disposition
    )
  }
}

/// A grant that exists on one side and not the other.
#[derive(Debug, FromQueryResult)]
pub struct ShadowDivergence {
  pub namespace:   String,
  pub object:      String,
  pub action:      String,
  pub subject:     String,
  /// `access_policies` — present in the legacy table, missing from the tuples.
  /// `relation_tuples` — present in the tuples, missing from the legacy table.
  pub only_in:     String,
  /// Why this difference is expected, or `NULL` when it is a real disagreement.
  pub explanation: Option<String>,
}

impl ShadowDivergence {
  fn render(&self) -> String {
    format!(
      "{:<10} {:<22} | {:<12} | {:<8} | {:<12} — {}",
      self.only_in,
      self.namespace,
      self.object,
      self.action,
      self.subject,
      self.explanation.as_deref().unwrap_or("UNEXPLAINED")
    )
  }
}

pub struct BackfillReport {
  pub inserted: u64,
  pub dropped:  Vec<PolicyDisposition>,
}

/// The classifier, as a pair of CTEs every statement below builds on.
///
/// Order inside the `CASE` is the whole of the logic, so it is written to be read top to bottom:
/// the things that must never be migrated come first, then the two object tiers that have a level
/// mapping, then the route-gate tier, and anything that reaches the bottom is a row the mapping
/// does not cover.
fn classify_cte() -> String {
  let service_namespaces = sql_text_array(SERVICE_NAMESPACES);
  let orphan_namespaces = sql_text_array(ORPHAN_NAMESPACES);

  format!(
    r#"
    WITH ap AS (
      SELECT
        p.namespace::text AS namespace,
        p.object::text    AS object,
        p.action::text[]  AS action,
        p."user"::text    AS subject,
        p.effect::text    AS effect,
        -- `who()` emits `user:admin` for an administrator session and `user:<id>` otherwise, so a
        -- catch-all row is "the admin grant" only for one of those two shapes.
        (p."user" = 'user:admin'
         OR EXISTS (SELECT 1 FROM users u WHERE p."user" = 'user:' || u.id AND u.is_admin)) AS subject_is_admin,
        -- The subject vocabulary `who()` can actually produce. Anything else matches no session,
        -- which is what site-app's reversed `admin:user` row is.
        (p."user" ~ '^(user|role|org|api):.+$') AS subject_parses
      FROM access_policies p
    ),
    classified AS (
      SELECT ap.*, CASE
        -- §9.1.1 — the exclusion mapping assumes there is no per-tuple deny to translate.
        WHEN ap.effect <> 'allow' THEN 'refuse:deny'

        -- §9.1.3 — a subject `who()` never emits matches nothing today, so dropping it changes no
        -- decision. Reported rather than silently discarded because it is evidence of a bad write
        -- path, not just a dead row.
        WHEN NOT ap.subject_parses THEN 'drop:dead-subject'

        -- §0.5 — seeded by site-app's initiator, gated on by nothing.
        WHEN ap.namespace = ANY ({orphan_namespaces}) THEN 'drop:orphan-namespace'

        -- §9.2 — superseded by `platform:root#superuser`, whose `flagged` branch is `users.is_admin`
        -- itself. Migrating it would put the admin grant in two places at once.
        WHEN ap.namespace = '*' AND ap.object = '*' AND '*' = ANY(ap.action) AND ap.subject_is_admin
          THEN 'drop:admin-catch-all'

        -- §0.2 — lab-app's site-sharing screen writes `action = {{'*'}}` for level Edit. This branch
        -- has to come before the wildcard refusal below or a migration would refuse to run against
        -- any lab-app database in which someone has pressed Edit.
        WHEN ap.namespace = 'lab-app:site' AND ap.object NOT IN ('service', '*')
          THEN CASE WHEN ap.subject LIKE 'user:%' OR ap.subject LIKE 'org:%'
                    THEN 'migrate:daqs_site' ELSE 'refuse:lab-app-site-subject' END

        -- §0.2's lesson, applied a second time. `object = 'service' AND action = {{'*'}}` means
        -- "every route action on this namespace" and nothing else — the object is `service`, so
        -- there is no object-wide power in it to guess at. That is exactly the four route-gate
        -- relations, so it has a translation and must be taken *before* the generic wildcard
        -- refusal below. A refusal that ran first would drop every such grant, which is the same
        -- mistake as refusing lab-app's `Edit = {{'*'}}` shares.
        WHEN ap.object = 'service' AND '*' = ANY(ap.action) AND ap.namespace <> '*'
          THEN CASE WHEN ap.namespace = ANY ({service_namespaces})
                    THEN 'migrate:service_wildcard' ELSE 'refuse:unknown-namespace' END

        -- §9.2 — a wildcard on a non-admin subject is not expressible. It needs a person to decide
        -- whether it meant `service:<ns>#operator` or `platform:root#admin`, and guessing either
        -- way silently changes what that account can reach.
        WHEN ap.namespace = '*' OR ap.object = '*' OR '*' = ANY(ap.action) THEN 'refuse:wildcard'

        WHEN ap.namespace = 'trainer:delegate' AND ap.object <> 'service'
          THEN CASE WHEN ap.action <@ ARRAY['assert']::text[]
                    THEN 'migrate:delegation' ELSE 'refuse:unmapped-delegation' END

        -- site-app encoded the tier in the namespace suffix: a numeric object is an organization id,
        -- anything else is a project slug.
        WHEN ap.namespace = 'site-app:project.row' AND ap.object ~ '^[0-9]{{1,9}}$'
          THEN 'migrate:antares_project_org'
        WHEN ap.namespace = 'site-app:project.row' THEN 'migrate:antares_project'

        WHEN ap.object = 'service' THEN CASE
          WHEN NOT (ap.namespace = ANY ({service_namespaces})) THEN 'refuse:unknown-namespace'
          WHEN NOT (ap.action <@ ARRAY['create','read','update','delete']::text[])
            THEN 'refuse:unknown-service-action'
          ELSE 'migrate:service' END

        -- §9.2 — `board_post` and `file` ownership rows written at runtime are discarded, because
        -- the replacement relations derive from `board_posts.user_id` / `files.user_id`, which are
        -- already populated for the same rows. Keeping both would create the second copy the
        -- design exists to remove.
        WHEN ap.namespace IN ('board_post', 'file') THEN 'drop:derived-ownership'

        ELSE 'refuse:unmapped-object-tier'
      END AS disposition
      FROM ap
    )
    "#
  )
}

/// `subject_type`, `subject_id`, `subject_relation` from a legacy `"user"` string.
///
/// `org:<n>` becomes `organization:<n>#member` — the old prefix means "any member of this
/// organization", which is a userset, not the organization object. Mapping it to a bare
/// `organization:<n>` would grant nothing at all, which is the failure site-app's dead `admin:user`
/// row already demonstrates.
const SUBJECT_COLUMNS: &str = r#"
  CASE WHEN c.subject LIKE 'org:%' THEN 'organization'
       WHEN c.subject LIKE 'api:%' THEN 'apikey'
       ELSE split_part(c.subject, ':', 1) END,
  substring(c.subject from position(':' in c.subject) + 1),
  CASE WHEN c.subject LIKE 'role:%' OR c.subject LIKE 'org:%' THEN 'member' ELSE '' END
"#;

const TUPLE_COLUMNS: &str = "relation_tuples (resource_type, resource_id, relation, subject_type, subject_id, subject_relation, created_by)";

pub fn sql_text_array(values: &[&str]) -> String {
  let items = values.iter().map(|v| format!("'{v}'")).collect::<Vec<_>>().join(",");
  format!("ARRAY[{items}]::text[]")
}

async fn table_exists<C: ConnectionTrait>(db: &C, name: &str) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Exists {
    present: bool,
  }

  let row = Exists::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "SELECT to_regclass($1) IS NOT NULL AS present",
    [name.into()],
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

async fn dispositions<C: ConnectionTrait>(db: &C, prefix: &str) -> Result<Vec<PolicyDisposition>, DbErr> {
  let sql = format!(
    "{} SELECT namespace, object, array_to_string(action, ',') AS action, subject, effect, disposition \
     FROM classified WHERE disposition LIKE '{prefix}%' ORDER BY namespace, object, subject",
    classify_cte()
  );

  PolicyDisposition::find_by_statement(Statement::from_string(db.get_database_backend(), sql))
    .all(db)
    .await
}

/// Runs the pre-flight of §9.1 and, if it passes, the backfill of §9.2.
///
/// Idempotent: every insert is `ON CONFLICT DO NOTHING` against the tuple's natural key, so
/// re-running it after fixing a refused row adds only what was missing.
pub async fn backfill<C: ConnectionTrait>(db: &C) -> Result<BackfillReport, DbErr> {
  let refusals = dispositions(db, "refuse:").await?;
  if !refusals.is_empty() {
    let rows = refusals.iter().map(PolicyDisposition::render).collect::<Vec<_>>().join("\n  ");
    return Err(DbErr::Custom(format!(
      "rebac_backfill refuses to run: {} access_policies row(s) cannot be translated without \
       changing what somebody can reach. Fix or delete them, then run the migration again.\n  {rows}",
      refusals.len()
    )));
  }

  let cte = classify_cte();
  let subject = SUBJECT_COLUMNS;
  let statements = [
    // Route-gate tier. One tuple per (namespace, action): the `action varchar[]` array becomes one
    // row per relation, which is what lets the index below serve an equality instead of `@>`.
    format!(
      "{cte} INSERT INTO {TUPLE_COLUMNS} \
       SELECT 'service', c.namespace, act, {subject}, 'migration' \
       FROM classified c, LATERAL unnest(c.action) AS act \
       WHERE c.disposition = 'migrate:service' ON CONFLICT DO NOTHING"
    ),
    // The same tier written with a wildcard action. Four relations, one per route action — the
    // wildcard is expanded rather than carried, because `relation` is a scalar and `{'*'}` is
    // precisely what the `@>` predicate a btree cannot serve was there to answer.
    format!(
      "{cte} INSERT INTO {TUPLE_COLUMNS} \
       SELECT 'service', c.namespace, act, {subject}, 'migration' \
       FROM classified c, LATERAL unnest(ARRAY['create','read','update','delete']::text[]) AS act \
       WHERE c.disposition = 'migrate:service_wildcard' ON CONFLICT DO NOTHING"
    ),
    // `trainer:delegate | <scope> | {assert}`. The scope object id keeps its exact string, because it
    // is compared for equality — lab-app must not be able to assert site-app's scope.
    format!(
      "{cte} INSERT INTO {TUPLE_COLUMNS} \
       SELECT 'delegation_scope', c.object, 'operator', {subject}, 'migration' \
       FROM classified c WHERE c.disposition = 'migrate:delegation' ON CONFLICT DO NOTHING"
    ),
    // lab-app's two sharing levels. `Edit` writes `{'*'}` and `Read` writes `{read}`, so the level —
    // not the action set — is what round-trips (§0.2).
    format!(
      "{cte} INSERT INTO {TUPLE_COLUMNS} \
       SELECT 'daqs_site', c.object, \
              CASE WHEN '*' = ANY(c.action) OR 'update' = ANY(c.action) THEN 'editor' ELSE 'viewer' END, \
              {subject}, 'migration' \
       FROM classified c WHERE c.disposition = 'migrate:daqs_site' ON CONFLICT DO NOTHING"
    ),
    // site-app's three levels. `owner` subsumes read and update by implication, so the extra actions
    // in the source row do not need tuples of their own.
    format!(
      "{cte} INSERT INTO {TUPLE_COLUMNS} \
       SELECT 'antares_project', c.object, \
              CASE WHEN 'owner' = ANY(c.action) OR '*' = ANY(c.action) THEN 'owner' \
                   WHEN 'update' = ANY(c.action) THEN 'editor' \
                   ELSE 'viewer' END, \
              {subject}, 'migration' \
       FROM classified c WHERE c.disposition = 'migrate:antares_project' ON CONFLICT DO NOTHING"
    ),
    // The same rows whose object is an organization id are the org tier, and land on
    // `organization#project_*` instead — three tiers, so a per-action org grant keeps its meaning.
    format!(
      "{cte} INSERT INTO {TUPLE_COLUMNS} \
       SELECT 'organization', c.object, \
              CASE WHEN 'owner' = ANY(c.action) OR '*' = ANY(c.action) THEN 'project_manager' \
                   WHEN 'update' = ANY(c.action) THEN 'project_editor' \
                   ELSE 'project_viewer' END, \
              {subject}, 'migration' \
       FROM classified c WHERE c.disposition = 'migrate:antares_project_org' ON CONFLICT DO NOTHING"
    ),
  ];

  let mut inserted = 0;
  for sql in statements {
    inserted += db
      .execute(Statement::from_string(db.get_database_backend(), sql))
      .await?
      .rows_affected();
  }

  let dropped = dispositions(db, "drop:").await?;
  for row in &dropped {
    tracing::warn!("rebac_backfill dropped a policy row: {}", row.render());
  }
  tracing::info!("rebac_backfill: {inserted} tuple(s) written, {} row(s) dropped", dropped.len());

  Ok(BackfillReport { inserted, dropped })
}

/// Projects `relation_tuples` back into the legacy `(namespace, object, action, user)` grain and
/// diffs it against `access_policies`.
///
/// This is the *data* half of the shadow comparison. The decision half — running both evaluators
/// over the same request and diffing the answers — needs the compiled evaluator and belongs to the
/// phase that introduces it. This one can run today, and it is what catches a mapping that lost a
/// grant, which is the failure that would make the decision comparison meaningless anyway.
///
/// A row with `explanation = None` is a real disagreement. Everything else is a difference the
/// design asked for, carried here so the output can be read as "clean" rather than "clean apart
/// from the ones I remember are fine".
pub async fn shadow_compare<C: ConnectionTrait>(db: &C) -> Result<Vec<ShadowDivergence>, DbErr> {
  let cte = classify_cte();
  let sql = format!(
    r#"
    {cte},
    legacy AS (
      -- A wildcard route-gate row is compared as the four grants it actually confers, not as the
      -- literal `*`. Diffing the literal would report four divergences for a row that translated
      -- exactly, which trains a reader to skim the output — the one thing this report must not do.
      SELECT c.namespace, c.object, act AS action, c.subject, c.disposition
      FROM classified c,
           LATERAL unnest(CASE WHEN c.disposition = 'migrate:service_wildcard'
                               THEN ARRAY['create','read','update','delete']::text[]
                               ELSE c.action END) AS act
      WHERE c.effect = 'allow'
    ),
    subj AS (
      SELECT rt.resource_type, rt.resource_id, rt.relation,
             CASE
               WHEN rt.subject_type = 'user'         AND rt.subject_relation = ''       THEN 'user:' || rt.subject_id
               WHEN rt.subject_type = 'role'         AND rt.subject_relation = 'member' THEN 'role:' || rt.subject_id
               WHEN rt.subject_type = 'organization' AND rt.subject_relation = 'member' THEN 'org:'  || rt.subject_id
               WHEN rt.subject_type = 'apikey'       AND rt.subject_relation = ''       THEN 'api:'  || rt.subject_id
             END AS subject
      FROM relation_tuples rt
      WHERE rt.expires_at IS NULL OR rt.expires_at > CURRENT_TIMESTAMP
    ),
    -- The inverse of every mapping in `backfill`. Written out rather than derived so that a change
    -- to one direction and not the other shows up as a divergence instead of cancelling out.
    projected AS (
      SELECT resource_id AS namespace, 'service' AS object, relation AS action, subject
      FROM subj WHERE resource_type = 'service'
      UNION ALL
      SELECT 'trainer:delegate', resource_id, 'assert', subject
      FROM subj WHERE resource_type = 'delegation_scope' AND relation = 'operator'
      UNION ALL
      SELECT 'lab-app:site', s.resource_id, a, s.subject
      FROM subj s, LATERAL unnest(CASE s.relation
             WHEN 'viewer' THEN ARRAY['read'] WHEN 'editor' THEN ARRAY['*'] END) AS a
      WHERE s.resource_type = 'daqs_site'
      UNION ALL
      SELECT 'site-app:project.row', s.resource_id, a, s.subject
      FROM subj s, LATERAL unnest(CASE s.relation
             WHEN 'owner'  THEN ARRAY['read','update','owner']
             WHEN 'editor' THEN ARRAY['read','update']
             WHEN 'viewer' THEN ARRAY['read'] END) AS a
      WHERE s.resource_type = 'antares_project'
      UNION ALL
      SELECT 'site-app:project.row', s.resource_id, a, s.subject
      FROM subj s, LATERAL unnest(CASE s.relation
             WHEN 'project_manager' THEN ARRAY['read','update','owner']
             WHEN 'project_editor'  THEN ARRAY['read','update']
             WHEN 'project_viewer'  THEN ARRAY['read'] END) AS a
      WHERE s.resource_type = 'organization' AND s.relation LIKE 'project\_%'
    )
    SELECT
      coalesce(l.namespace, p.namespace) AS namespace,
      coalesce(l.object, p.object)       AS object,
      coalesce(l.action, p.action)       AS action,
      coalesce(l.subject, p.subject)     AS subject,
      CASE WHEN p.namespace IS NULL THEN 'access_policies' ELSE 'relation_tuples' END AS only_in,
      CASE
        WHEN l.disposition LIKE 'drop:%' THEN l.disposition
        -- The object tiers map a *level*, not an action set (§0.2, §9.5): `{{read,update}}` and
        -- `{{'*'}}` both mean editor, and editor implies both. So the two sides differing on the
        -- action grain is the mapping working, not failing.
        WHEN l.disposition IN ('migrate:daqs_site', 'migrate:antares_project', 'migrate:antares_project_org')
          THEN 'level mapping (' || l.disposition || '): the relation round-trips, the action set need not'
        WHEN l.namespace IS NULL AND p.namespace IN ('lab-app:site', 'site-app:project.row')
          THEN 'level mapping: the relation implies an action the source row did not list'
      END AS explanation
    FROM legacy l
    FULL OUTER JOIN projected p
      ON l.namespace = p.namespace AND l.object = p.object
     AND l.action = p.action AND l.subject IS NOT DISTINCT FROM p.subject
    WHERE l.namespace IS NULL OR p.namespace IS NULL
    ORDER BY 1, 2, 3, 4
    "#
  );

  ShadowDivergence::find_by_statement(Statement::from_string(db.get_database_backend(), sql))
    .all(db)
    .await
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // dive links sumida for its types and has never had an `access_policies` table. Nothing to
    // translate there, and erroring would block a migration run that is otherwise fine.
    if !table_exists(db, "access_policies").await? {
      tracing::info!("rebac_backfill: no access_policies table — nothing to translate");
      return Ok(());
    }

    backfill(db).await?;

    // Report, do not fail. At this point every legacy grant that has a mapping is in both stores;
    // an unexplained difference here means the mapping is wrong, and that is a thing to read, not
    // a reason to roll back a migration that changed no behaviour.
    for divergence in shadow_compare(db).await? {
      match divergence.explanation {
        Some(_) => tracing::info!("rebac shadow (expected): {}", divergence.render()),
        None => tracing::warn!("rebac shadow DIVERGENCE: {}", divergence.render()),
      }
    }

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // Only what this migration wrote. `created_by` is 'migration' for every backfilled tuple and
    // nothing else writes that value, so a tuple created by hand after the fact survives a
    // rollback.
    db.execute_unprepared("DELETE FROM relation_tuples WHERE created_by = 'migration';")
      .await?;

    Ok(())
  }
}
