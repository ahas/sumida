use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

use super::rebac_backfill::{ORPHAN_NAMESPACES, SERVICE_NAMESPACES, sql_text_array};

/// The `access_policies` → `relation_tuples` mirror (design §14 step 4, Phase 0).
///
/// `rebac_backfill` translated the rows that existed when it ran. This keeps the two stores in step
/// afterwards, for as long as `ap.rs` is still the authority and things keep writing policy rows
/// through it — and there are five such writers across three products, two of them user-facing
/// screens in a live service (`board_post/mod.rs:448`, `fs/file.rs:134`,
/// `site-app/service/project/{mod.rs:270,shares.rs:150}`, lab-app's `site/mod.rs:186`). Without a
/// mirror the tuple store would be correct on the day of the backfill and progressively wrong
/// after it, and the shadow comparison would be measuring the gap rather than the mapping.
///
/// **Two properties this trigger must have, and both are the opposite of what a mirror usually
/// wants.**
///
/// * It **never fails a write.** Nothing reads the tuples yet, so a mirroring failure costs nothing
///   — while an exception here would abort an `INSERT INTO access_policies` that works today, which
///   is a behaviour change in a phase whose whole claim is that it makes none. A foreign-key
///   violation (a relation the vocabulary does not declare) is caught and warned about.
/// * It **mirrors only what the backfill would have migrated.** A row the backfill refuses is a row
///   a person has to decide about; inventing a tuple for it here would make that decision silently
///   and in the direction nobody chose. Everything else is skipped.
///
/// The dispositions are `rebac_backfill`'s, re-expressed for one row at a time. They are duplicated
/// rather than shared because the backfill is a set-based statement and this is a row trigger — so
/// `rebac_mirror_matches_the_backfill` in `api/tests/authz_equivalence.rs` runs both over the same
/// rows and asserts they agree, which is the only thing that makes the duplication safe.
#[derive(DeriveMigrationName)]
pub struct Migration;

async fn table_exists<C: ConnectionTrait>(db: &C, name: &str) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Present {
    present: bool,
  }

  let row = Present::find_by_statement(Statement::from_sql_and_values(
    db.get_database_backend(),
    "SELECT to_regclass($1) IS NOT NULL AS present",
    [name.into()],
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

/// `created_by` values that mark a tuple as having come from `access_policies`.
///
/// The mirror removes only these. `migration` is the backfill's stamp and `access_policies` is this
/// trigger's; a tuple written by hand or by `Authz::write` carries the granter's subject key and
/// survives, because deleting a legacy policy row must not revoke a grant nobody made through it.
const MIRRORED_BY: &str = "('migration', 'access_policies')";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // dive links sumida for its types and has never had an `access_policies` table.
    if !table_exists(db, "access_policies").await? {
      tracing::info!("rebac_mirror: no access_policies table — nothing to mirror");
      return Ok(());
    }

    let services = sql_text_array(SERVICE_NAMESPACES);
    let orphans = sql_text_array(ORPHAN_NAMESPACES);

    // The subject triple, from a legacy `"user"` string. `org:<n>` becomes
    // `organization:<n>#member` — the old prefix means "any member of this organization", which is
    // a userset. Mapping it to a bare `organization:<n>` would grant nothing at all, which is
    // exactly what site-app's reversed `admin:user` row already demonstrates.
    db.execute_unprepared(
      r#"
      CREATE OR REPLACE FUNCTION rebac_mirror_subject(p_user text)
      RETURNS TABLE (subject_type text, subject_id text, subject_relation text) AS $$
      BEGIN
        IF p_user !~ '^(user|role|org|api):.+$' THEN
          RETURN;   -- a subject `who()` never emits matches nothing today; mirroring it would be a lie
        END IF;

        RETURN QUERY SELECT
          CASE WHEN p_user LIKE 'org:%' THEN 'organization'
               WHEN p_user LIKE 'api:%' THEN 'apikey'
               ELSE split_part(p_user, ':', 1) END,
          substring(p_user from position(':' in p_user) + 1),
          CASE WHEN p_user LIKE 'role:%' OR p_user LIKE 'org:%' THEN 'member' ELSE '' END;
      END; $$ LANGUAGE plpgsql STABLE;
      "#,
    )
    .await?;

    // One `access_policies` row in, zero or more tuples out. Ordered exactly as the backfill's
    // `CASE` is ordered, because the order *is* the logic — most importantly, lab-app's site rows are
    // matched before the wildcard refusal, since its sharing screen writes `action = {'*'}` for
    // level Edit and a wildcard rule that ran first would drop every lab-app share.
    db.execute_unprepared(&format!(
      r#"
      CREATE OR REPLACE FUNCTION rebac_mirror_map(
        p_namespace text, p_object text, p_action text[], p_user text, p_effect text
      ) RETURNS TABLE (resource_type text, resource_id text, relation text) AS $$
      BEGIN
        -- The exclusion mapping assumes there is no per-tuple deny to translate, and no code path
        -- in any of the three products writes one.
        IF p_effect <> 'allow' THEN RETURN; END IF;

        -- Seeded by site-app's initiator, gated on by nothing.
        IF p_namespace = ANY ({orphans}) THEN RETURN; END IF;

        IF p_namespace = 'lab-app:site' AND p_object NOT IN ('service', '*') THEN
          IF p_user LIKE 'user:%' OR p_user LIKE 'org:%' THEN
            RETURN QUERY SELECT 'daqs_site', p_object,
              CASE WHEN '*' = ANY(p_action) OR 'update' = ANY(p_action) THEN 'editor'
                   ELSE 'viewer' END;
          END IF;
          RETURN;
        END IF;

        -- `object = 'service' AND action = {{'*'}}` means "every route action on this namespace"
        -- and nothing more, because the object is `service`. That is the four route-gate relations
        -- exactly, so it is translated here — *before* the generic wildcard skip below, for the
        -- same reason lab-app's site rows are: a refusal that runs before the branch which knows what
        -- the wildcard meant drops a grant that had an exact translation.
        IF p_object = 'service' AND '*' = ANY(p_action) AND p_namespace <> '*' THEN
          IF p_namespace = ANY ({services}) THEN
            RETURN QUERY SELECT 'service', p_namespace, a
              FROM unnest(ARRAY['create','read','update','delete']::text[]) AS a;
          END IF;
          RETURN;
        END IF;

        -- A wildcard on any column is not expressible, and the admin catch-all is superseded by
        -- `platform:root#superuser` — whose `flagged` branch is `users.is_admin` itself, so
        -- mirroring it would put the admin grant in two places at once. Both are skipped.
        IF p_namespace = '*' OR p_object = '*' OR '*' = ANY(p_action) THEN RETURN; END IF;

        IF p_namespace = 'trainer:delegate' AND p_object <> 'service' THEN
          IF p_action <@ ARRAY['assert']::text[] THEN
            -- The scope object id keeps its exact string: it is compared for equality, which is
            -- what stops lab-app asserting site-app's scope.
            RETURN QUERY SELECT 'delegation_scope', p_object, 'operator';
          END IF;
          RETURN;
        END IF;

        -- site-app encoded the tier in the object: a numeric object is an organization id, anything
        -- else is a project slug.
        IF p_namespace = 'site-app:project.row' THEN
          IF p_object ~ '^[0-9]{{1,9}}$' THEN
            RETURN QUERY SELECT 'organization', p_object,
              CASE WHEN 'owner'  = ANY(p_action) THEN 'project_manager'
                   WHEN 'update' = ANY(p_action) THEN 'project_editor'
                   ELSE 'project_viewer' END;
          ELSE
            RETURN QUERY SELECT 'antares_project', p_object,
              CASE WHEN 'owner'  = ANY(p_action) THEN 'owner'
                   WHEN 'update' = ANY(p_action) THEN 'editor'
                   ELSE 'viewer' END;
          END IF;
          RETURN;
        END IF;

        IF p_object = 'service' THEN
          IF p_namespace = ANY ({services})
             AND p_action <@ ARRAY['create','read','update','delete']::text[] THEN
            -- One tuple per action: the `action varchar[]` array becomes one row per relation,
            -- which is what lets the index serve an equality instead of the `@>` a btree cannot
            -- answer.
            RETURN QUERY SELECT 'service', p_namespace, a FROM unnest(p_action) AS a;
          END IF;
          RETURN;
        END IF;

        -- `board_post` and `file` ownership rows written at runtime are deliberately not mirrored:
        -- the replacement relations derive from `board_posts.user_id` / `files.user_id`, which are
        -- already populated for the same rows. Everything else is a shape the mapping does not
        -- cover, and inventing a tuple for it is the silent migration this phase must not do.
        RETURN;
      END; $$ LANGUAGE plpgsql STABLE;
      "#
    ))
    .await?;

    db.execute_unprepared(&format!(
      r#"
      CREATE OR REPLACE FUNCTION tg_rebac_mirror_access_policy() RETURNS trigger AS $$
      DECLARE
        tuple_row   record;
        subject_row record;
      BEGIN
        IF TG_OP IN ('UPDATE', 'DELETE') THEN
          FOR subject_row IN SELECT * FROM rebac_mirror_subject(OLD."user") LOOP
            FOR tuple_row IN
              SELECT * FROM rebac_mirror_map(OLD.namespace, OLD.object, OLD.action, OLD."user", OLD.effect)
            LOOP
              DELETE FROM relation_tuples t
               WHERE t.resource_type    = tuple_row.resource_type
                 AND t.resource_id      = tuple_row.resource_id
                 AND t.relation         = tuple_row.relation
                 AND t.subject_type     = subject_row.subject_type
                 AND t.subject_id       = subject_row.subject_id
                 AND t.subject_relation = subject_row.subject_relation
                 AND t.created_by IN {MIRRORED_BY};
            END LOOP;
          END LOOP;
        END IF;

        IF TG_OP IN ('INSERT', 'UPDATE') THEN
          FOR subject_row IN SELECT * FROM rebac_mirror_subject(NEW."user") LOOP
            FOR tuple_row IN
              SELECT * FROM rebac_mirror_map(NEW.namespace, NEW.object, NEW.action, NEW."user", NEW.effect)
            LOOP
              BEGIN
                INSERT INTO relation_tuples
                  (resource_type, resource_id, relation,
                   subject_type, subject_id, subject_relation, created_by)
                VALUES
                  (tuple_row.resource_type, tuple_row.resource_id, tuple_row.relation,
                   subject_row.subject_type, subject_row.subject_id, subject_row.subject_relation,
                   'access_policies')
                ON CONFLICT DO NOTHING;
              EXCEPTION WHEN foreign_key_violation THEN
                -- Nothing reads the tuples yet, so a failed mirror costs nothing — while letting
                -- the exception out would abort a write to `access_policies` that works today.
                RAISE WARNING 'rebac mirror: no vocabulary for %#%, tuple not mirrored',
                  tuple_row.resource_type, tuple_row.relation;
              END;
            END LOOP;
          END LOOP;
        END IF;

        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_rebac_mirror ON access_policies;
      CREATE TRIGGER trg_rebac_mirror
        AFTER INSERT OR UPDATE OR DELETE ON access_policies
        FOR EACH ROW EXECUTE FUNCTION tg_rebac_mirror_access_policy();
      "#
    ))
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if !table_exists(db, "access_policies").await? {
      return Ok(());
    }

    db.execute_unprepared(
      r#"
      DROP TRIGGER IF EXISTS trg_rebac_mirror ON access_policies;
      DROP FUNCTION IF EXISTS tg_rebac_mirror_access_policy();
      DROP FUNCTION IF EXISTS rebac_mirror_map(text, text, text[], text, text);
      DROP FUNCTION IF EXISTS rebac_mirror_subject(text);
      DELETE FROM relation_tuples WHERE created_by = 'access_policies';
      "#,
    )
    .await?;

    Ok(())
  }
}
