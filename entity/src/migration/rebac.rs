//! ReBAC data layer — the tuple table, the rule-vocabulary projection, and the closure tables
//! that materialise the authorization-bearing trees.

use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// Also lays down the sumida-side columns and indexes the compiled rule set reads, since a rule
/// that reads a column which does not exist is reported as drift by `Authz::boot` and refuses to
/// start. Any further column a rule needs belongs to whichever definition declares it, and lands in
/// that declarer's own migration.
///
/// Depends on `organizations` (init); the board half additionally needs `board_groups` and `boards`
/// (board_system), so consumers that have boards must register this after that migration.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Whether the board half applies to *this database*.
///
/// Deliberately a runtime probe and not `#[cfg(feature = "board")]`. Cargo unifies features across
/// the whole dependency graph, so `entity` ends up compiled with `board` on for a build that merely
/// links something which wants it — a test binary pulling in a dev-dependency is exactly that —
/// while the database that build migrates has no board tables at all. A compile-time gate would
/// therefore create `board_group_closure` against a `board_groups` that does not exist.
async fn has_board_groups<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
  #[derive(FromQueryResult)]
  struct Present {
    present: bool,
  }

  let row = Present::find_by_statement(Statement::from_string(
    db.get_database_backend(),
    "SELECT to_regclass('board_groups') IS NOT NULL AS present".to_owned(),
  ))
  .one(db)
  .await?;

  Ok(row.map(|r| r.present).unwrap_or(false))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // R1's flag branch is an uncorrelated `EXISTS (SELECT 1 FROM users WHERE is_admin AND id =
    // ANY($2))` that Postgres hoists into an InitPlan, so it wants an index rather than a
    // sequential scan once per statement. Partial, because a query only ever asks "is this account
    // an admin"; the non-admin rows would be dead weight.
    db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_users__is_admin ON users (id) WHERE is_admin;")
      .await?;

    // ── the rule vocabulary ─────────────────────────────────────────────────────────────────
    //
    // These two tables are a *projection* of the compiled rule registry; the evaluator never reads
    // them. They exist here for two reasons: the foreign key below, which is the only thing that
    // can stop a raw-SQL or `ActiveModel` writer inserting a relation nobody declared, and
    // `GET /authz/schema`, which the /relation-tuples screen reads.
    db.execute_unprepared(
      r#"
      CREATE TABLE IF NOT EXISTS relation_definitions (
        resource_type   TEXT    NOT NULL,
        relation        TEXT    NOT NULL,
        kind            TEXT    NOT NULL CHECK (kind IN ('stored','derived')),
        subject_types   TEXT[]  NOT NULL DEFAULT '{}',
        allows_wildcard BOOLEAN NOT NULL DEFAULT FALSE,
        source          JSONB,
        CONSTRAINT pk_relation_definitions PRIMARY KEY (resource_type, relation)
      );

      CREATE TABLE IF NOT EXISTS permission_definitions (
        resource_type TEXT  NOT NULL,
        permission    TEXT  NOT NULL,
        rewrite       JSONB NOT NULL,
        CONSTRAINT pk_permission_definitions PRIMARY KEY (resource_type, permission)
      );

      COMMENT ON TABLE relation_definitions IS
        'Read-only projection of the compiled rule registry. Rewritten at boot; do not edit by hand.';
      COMMENT ON TABLE permission_definitions IS
        'Read-only projection of the compiled rule registry. For introspection only.';
      "#,
    )
    .await?;

    // Bootstrap vocabulary — **only** what a database needs before anything has booted.
    //
    // This is not the rule set: it is the five relations an initial seed writes, so the foreign key
    // below can be in force from the moment the tuple table exists. `Authz::sync_schema` rewrites
    // this table from the compiled registry at boot and is the single source of truth thereafter,
    // which is why nothing more is seeded here — a longer list in a migration is a second copy of
    // the rules that drifts, and a migration cannot know what a consumer's rules declare anyway.
    db.execute_unprepared(
      r#"
      INSERT INTO relation_definitions (resource_type, relation, kind, subject_types) VALUES
        ('service',          'create',   'stored', '{user,role#member,organization#member,apikey}'),
        ('service',          'read',     'stored', '{user,role#member,organization#member,apikey}'),
        ('service',          'update',   'stored', '{user,role#member,organization#member,apikey}'),
        ('service',          'delete',   'stored', '{user,role#member,organization#member,apikey}'),
        ('delegation_scope', 'operator', 'stored', '{user,role#member,apikey}')
      ON CONFLICT DO NOTHING;
      "#,
    )
    .await?;

    // ── relation_tuples ─────────────────────────────────────────────────────────────────────
    //
    // Every column below is load-bearing, and the reason is the same each time — a grant has to be
    // identified exactly, matched by an index, and revoked without leaving a copy behind:
    //
    //   · resource_type / resource_id are separate. One untyped object column ends up carrying a
    //     row id for one namespace, an organization id for another, a slug for a third and a
    //     literal marker for route gates, disambiguated by naming convention alone.
    //   · `relation` is one scalar per row, never an array. An array column is matched with
    //     containment (`@>`), which a btree cannot answer, so an index over it is usable only for
    //     the columns *before* it and the subject column is never index-filtered. Plain equality
    //     keeps the index below usable end to end. Do not reintroduce an array column here.
    //   · subject_type/subject_id/subject_relation is Zanzibar's subject triple. The third
    //     component is what makes a subject a *set*: `role:1#member` is "every member of role 1"
    //     said out loud, and `subject_id = '*'` is the public wildcard without any string parsing.
    //   · expires_at — a grant can be given a lifetime.
    //   · created_by — who granted it. Without a grantor there is nothing to check a granter's own
    //     authority against.
    //   · uq_relation_tuples__tuple — a non-unique index in this position admits a duplicate grant,
    //     and then a revocation that deletes one row leaves the other behind.
    //
    // `resource_id_int` bounds the digit count rather than testing `^[0-9]+$`. An unbounded
    // predicate admits a numeric string wider than `integer`, and the cast then raises 22003 at
    // INSERT time on a generated column the writer never mentioned. `{1,9}` cannot overflow.
    db.execute_unprepared(
      r#"
      CREATE TABLE IF NOT EXISTS relation_tuples (
        id               BIGINT      GENERATED ALWAYS AS IDENTITY,
        resource_type    TEXT        NOT NULL,
        resource_id      TEXT        NOT NULL,
        relation         TEXT        NOT NULL,
        subject_type     TEXT        NOT NULL,
        subject_id       TEXT        NOT NULL,
        subject_relation TEXT        NOT NULL DEFAULT '',
        subject_key      TEXT        NOT NULL GENERATED ALWAYS AS (
                           subject_type || ':' || subject_id ||
                           CASE WHEN subject_relation = '' THEN '' ELSE '#' || subject_relation END
                         ) STORED,
        resource_id_int  INTEGER     GENERATED ALWAYS AS (
                           CASE WHEN resource_id ~ '^[0-9]{1,9}$' THEN resource_id::integer END
                         ) STORED,
        expires_at       TIMESTAMPTZ,
        created_at       TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
        created_by       TEXT        NOT NULL DEFAULT 'system',
        CONSTRAINT pk_relation_tuples PRIMARY KEY (id),
        CONSTRAINT uq_relation_tuples__tuple
          UNIQUE (resource_type, resource_id, relation, subject_type, subject_id, subject_relation),
        CONSTRAINT fk_relation_tuples__relation
          FOREIGN KEY (resource_type, relation)
          REFERENCES relation_definitions (resource_type, relation) ON UPDATE CASCADE
      );
      "#,
    )
    .await?;

    // Indexes. Column order follows the access pattern the compiled rules emit:
    // resource_type and relation are compile-time literals, resource_id comes from the row under
    // test, and subject_key is matched against one bound text[].
    db.execute_unprepared(
      r#"
      CREATE INDEX IF NOT EXISTS idx_rt__lookup
        ON relation_tuples (resource_type, relation, resource_id, subject_key)
        INCLUDE (expires_at);

      -- Org-scope probe. Partial, because resource_id_int is only ever joined for organization
      -- rows; every other resource type has a text id and no business in this index.
      CREATE INDEX IF NOT EXISTS idx_rt__org_scope
        ON relation_tuples (relation, subject_key, resource_id_int)
        WHERE resource_type = 'organization';

      -- "what does this subject hold" — /me/permissions, the role form, the api-key form,
      -- revocation.
      CREATE INDEX IF NOT EXISTS idx_rt__subject
        ON relation_tuples (subject_key, resource_type, relation);

      -- "who holds this on this object" — the direction the sharing screens read. Not redundant
      -- with idx_rt__lookup: those screens want every subject on one object, which the 4-column
      -- index can only serve by also scanning subject_key. The INCLUDE keeps it index-only.
      CREATE INDEX IF NOT EXISTS idx_rt__object
        ON relation_tuples (resource_type, resource_id, relation) INCLUDE (subject_key);

      CREATE INDEX IF NOT EXISTS idx_rt__expiry
        ON relation_tuples (expires_at) WHERE expires_at IS NOT NULL;
      "#,
    )
    .await?;

    // ── org_closure ─────────────────────────────────────────────────────────────────────────
    //
    // "An organization manager reaches its descendants, to arbitrary depth" is otherwise a
    // recursive walk run per row, whose cost accumulates by N×depth and which every caller that
    // needs it writes again, sometimes in the opposite direction. Materialised here it is one
    // bounded join, written once.
    //
    // Maintenance is a **full rebuild** in a statement-level trigger, not an incremental update.
    // Organizations number in the hundreds and change rarely; incremental closure maintenance
    // under subtree re-parenting is a classic source of silent corruption, and corruption here is
    // a silent change of who can see what.
    db.execute_unprepared(
      r#"
      CREATE TABLE IF NOT EXISTS org_closure (
        descendant_id INTEGER NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
        ancestor_id   INTEGER NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
        depth         INTEGER NOT NULL,
        CONSTRAINT pk_org_closure PRIMARY KEY (descendant_id, ancestor_id)
      );

      -- descendant_id leads the PK because the point check asks "given this resource's org, which
      -- orgs sit above it". The list direction asks the opposite, and gets its own index.
      CREATE INDEX IF NOT EXISTS idx_org_closure__ancestor
        ON org_closure (ancestor_id, descendant_id);
      "#,
    )
    .await?;

    // The rebuild is a plain function rather than the trigger function itself so the migration can
    // seed the closure with the same code the trigger runs — one body, no chance of the seed and
    // the maintenance disagreeing.
    db.execute_unprepared(
      r#"
      CREATE OR REPLACE FUNCTION rebuild_org_closure() RETURNS void AS $$
      DECLARE max_depth integer;
      BEGIN
        -- Serialise concurrent rebuilds. Two sessions re-parenting at once would otherwise
        -- interleave DELETE and INSERT and leave a partial closure behind.
        PERFORM pg_advisory_xact_lock(hashtext('org_closure'));

        -- Two statements, not one data-modifying CTE: in a single statement the INSERT reads the
        -- pre-DELETE snapshot and collides on the primary key.
        DELETE FROM org_closure;
        INSERT INTO org_closure (descendant_id, ancestor_id, depth)
        WITH RECURSIVE walk AS (
          SELECT id AS ancestor_id, id AS descendant_id, 0 AS depth FROM organizations
          UNION ALL
          SELECT w.ancestor_id, o.id, w.depth + 1
            FROM walk w JOIN organizations o ON o.parent_id = w.descendant_id
           WHERE w.depth < 64
        )
        SELECT descendant_id, ancestor_id, depth FROM walk;

        SELECT max(depth) INTO max_depth FROM org_closure;
        IF max_depth >= 64 THEN
          -- Truncating the walk would silently NARROW authority — a manager would stop reaching
          -- the deepest part of their own subtree and nothing would say so. Refuse instead.
          RAISE EXCEPTION 'organization hierarchy exceeds the supported depth of 64';
        END IF;
      END; $$ LANGUAGE plpgsql;

      CREATE OR REPLACE FUNCTION tg_rebuild_org_closure() RETURNS trigger AS $$
      BEGIN
        PERFORM rebuild_org_closure();
        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      -- A cycle is rejected at write time rather than tolerated by the depth cap: the cap would
      -- turn a cycle into a 64-row-deep closure and an exception from the rebuild, i.e. an error
      -- pointing at the wrong statement.
      CREATE OR REPLACE FUNCTION tg_reject_org_cycle() RETURNS trigger AS $$
      BEGIN
        IF NEW.parent_id IS NULL THEN
          RETURN NEW;
        END IF;
        IF NEW.parent_id = NEW.id THEN
          RAISE EXCEPTION 'organization % cannot be its own parent', NEW.id;
        END IF;
        IF EXISTS (
          SELECT 1 FROM org_closure WHERE ancestor_id = NEW.id AND descendant_id = NEW.parent_id
        ) THEN
          RAISE EXCEPTION 'organization % cannot be parented under its own descendant %',
            NEW.id, NEW.parent_id;
        END IF;
        RETURN NEW;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_org_cycle ON organizations;
      CREATE TRIGGER trg_org_cycle
        BEFORE INSERT OR UPDATE OF parent_id ON organizations
        FOR EACH ROW EXECUTE FUNCTION tg_reject_org_cycle();

      -- DELETE is in the event list because `organizations.parent_id` is ON DELETE SET NULL:
      -- removing a node re-parents its children to NULL, which changes the closure of a whole
      -- subtree, not just of the deleted row.
      DROP TRIGGER IF EXISTS trg_org_closure ON organizations;
      CREATE TRIGGER trg_org_closure
        AFTER INSERT OR DELETE OR UPDATE OF parent_id ON organizations
        FOR EACH STATEMENT EXECUTE FUNCTION tg_rebuild_org_closure();

      -- The rebuild takes an advisory lock so two rebuilds cannot interleave their `DELETE` and
      -- `INSERT`. Taken inside the rebuild it would come *after* the row locks the writing
      -- transaction already holds, and two transactions writing different rows of the same tree
      -- would then take their locks in opposite orders — each holding a row the other's rebuild has
      -- to read for its foreign keys, each wanting the advisory lock the other holds. Postgres
      -- breaks that cycle by killing one, and the caller sees a 500 on an ordinary write.
      --
      -- Taking it in a `BEFORE … FOR EACH STATEMENT` trigger fixes the order for every writer: the
      -- advisory lock always first, rows always second. Advisory locks are re-entrant within a
      -- transaction, so the rebuild still takes it and still holds it for anything reaching the
      -- rebuild by another route. The lock is in the database rather than in the callers because a
      -- caller that forgets it reopens the hole silently, and nothing about a write to
      -- `organizations` announces that a whole table is about to be rebuilt underneath it.
      CREATE OR REPLACE FUNCTION tg_lock_org_closure() RETURNS trigger AS $$
      BEGIN
        PERFORM pg_advisory_xact_lock(hashtext('org_closure'));
        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_org_closure_lock ON organizations;
      CREATE TRIGGER trg_org_closure_lock
        BEFORE INSERT OR DELETE OR UPDATE OF parent_id ON organizations
        FOR EACH STATEMENT EXECUTE FUNCTION tg_lock_org_closure();

      SELECT rebuild_org_closure();
      "#,
    )
    .await?;

    // ── board_group_closure ─────────────────────────────────────────────────────────────────
    //
    // Of the three self-referencing trees in this estate only two carry an authorization edge:
    //
    //   · organizations.parent_id  — R2 is defined in terms of it.
    //   · board_groups.parent_id   — `authorize_post_mutation` composes a decision across the
    //                                post's group and board, and a grant on a parent group has to
    //                                reach the boards under it.
    //   · board_posts.parent_id    — **no closure, on purpose.** It is the comment thread, and a
    //                                comment's permission is derived from its `board_id`, not from
    //                                its parent post; the only traversal of the column anywhere is
    //                                the delete cascade. A closure over it would materialise a
    //                                relationship no rule reads, and would invite one to be
    //                                written by accident.
    if !has_board_groups(db).await? {
      return Ok(());
    }

    db.execute_unprepared(
      r#"
      CREATE TABLE IF NOT EXISTS board_group_closure (
        descendant_id INTEGER NOT NULL REFERENCES board_groups(id) ON DELETE CASCADE,
        ancestor_id   INTEGER NOT NULL REFERENCES board_groups(id) ON DELETE CASCADE,
        depth         INTEGER NOT NULL,
        CONSTRAINT pk_board_group_closure PRIMARY KEY (descendant_id, ancestor_id)
      );

      CREATE INDEX IF NOT EXISTS idx_board_group_closure__ancestor
        ON board_group_closure (ancestor_id, descendant_id);

      CREATE OR REPLACE FUNCTION rebuild_board_group_closure() RETURNS void AS $$
      DECLARE max_depth integer;
      BEGIN
        PERFORM pg_advisory_xact_lock(hashtext('board_group_closure'));

        DELETE FROM board_group_closure;
        INSERT INTO board_group_closure (descendant_id, ancestor_id, depth)
        WITH RECURSIVE walk AS (
          SELECT id AS ancestor_id, id AS descendant_id, 0 AS depth FROM board_groups
          UNION ALL
          SELECT w.ancestor_id, g.id, w.depth + 1
            FROM walk w JOIN board_groups g ON g.parent_id = w.descendant_id
           WHERE w.depth < 64
        )
        SELECT descendant_id, ancestor_id, depth FROM walk;

        SELECT max(depth) INTO max_depth FROM board_group_closure;
        IF max_depth >= 64 THEN
          RAISE EXCEPTION 'board group hierarchy exceeds the supported depth of 64';
        END IF;
      END; $$ LANGUAGE plpgsql;

      CREATE OR REPLACE FUNCTION tg_rebuild_board_group_closure() RETURNS trigger AS $$
      BEGIN
        PERFORM rebuild_board_group_closure();
        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      CREATE OR REPLACE FUNCTION tg_reject_board_group_cycle() RETURNS trigger AS $$
      BEGIN
        IF NEW.parent_id IS NULL THEN
          RETURN NEW;
        END IF;
        IF NEW.parent_id = NEW.id THEN
          RAISE EXCEPTION 'board group % cannot be its own parent', NEW.id;
        END IF;
        IF EXISTS (
          SELECT 1 FROM board_group_closure
           WHERE ancestor_id = NEW.id AND descendant_id = NEW.parent_id
        ) THEN
          RAISE EXCEPTION 'board group % cannot be parented under its own descendant %',
            NEW.id, NEW.parent_id;
        END IF;
        RETURN NEW;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_board_group_cycle ON board_groups;
      CREATE TRIGGER trg_board_group_cycle
        BEFORE INSERT OR UPDATE OF parent_id ON board_groups
        FOR EACH ROW EXECUTE FUNCTION tg_reject_board_group_cycle();

      DROP TRIGGER IF EXISTS trg_board_group_closure ON board_groups;
      CREATE TRIGGER trg_board_group_closure
        AFTER INSERT OR DELETE OR UPDATE OF parent_id ON board_groups
        FOR EACH STATEMENT EXECUTE FUNCTION tg_rebuild_board_group_closure();

      -- The lock ordering the organization half explains, for the other tree.
      CREATE OR REPLACE FUNCTION tg_lock_board_group_closure() RETURNS trigger AS $$
      BEGIN
        PERFORM pg_advisory_xact_lock(hashtext('board_group_closure'));
        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_board_group_closure_lock ON board_groups;
      CREATE TRIGGER trg_board_group_closure_lock
        BEFORE INSERT OR DELETE OR UPDATE OF parent_id ON board_groups
        FOR EACH STATEMENT EXECUTE FUNCTION tg_lock_board_group_closure();

      SELECT rebuild_board_group_closure();
      "#,
    )
    .await?;

    // ── the column the board rule reads ─────────────────────────────────────────────────────
    //
    // **Nullable on purpose.** It is how "boards stay general-purpose" lives in the model instead
    // of in a config flag: a public-board deployment leaves it NULL and the `org->manage_board`
    // branch of the board rule is statically dead, while a business deployment sets it and
    // organization managers reach their own boards. A `NOT NULL` column with a default would force
    // every deployment into the second shape.
    db.execute_unprepared(
      r#"
      ALTER TABLE boards
        ADD COLUMN IF NOT EXISTS organization_id INTEGER NULL
          REFERENCES organizations(id) ON DELETE SET NULL;

      -- Partial: NULL means "this board is not organization-scoped", which is the common case in a
      -- public-board deployment and never a value the rule looks up.
      CREATE INDEX IF NOT EXISTS idx_boards__organization_id
        ON boards (organization_id) WHERE organization_id IS NOT NULL;

      COMMENT ON COLUMN boards.organization_id IS
        'Optional owning organization. NULL leaves the board outside organization scoping; the rule''s org branch is then dead.';
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if has_board_groups(db).await? {
      db.execute_unprepared(
        r#"
      DROP INDEX IF EXISTS idx_boards__organization_id;
      ALTER TABLE boards DROP COLUMN IF EXISTS organization_id;

      DROP TRIGGER IF EXISTS trg_board_group_closure_lock ON board_groups;
      DROP FUNCTION IF EXISTS tg_lock_board_group_closure();
      DROP TRIGGER IF EXISTS trg_board_group_closure ON board_groups;
      DROP TRIGGER IF EXISTS trg_board_group_cycle ON board_groups;
      DROP FUNCTION IF EXISTS tg_rebuild_board_group_closure();
      DROP FUNCTION IF EXISTS tg_reject_board_group_cycle();
      DROP FUNCTION IF EXISTS rebuild_board_group_closure();
      DROP TABLE IF EXISTS board_group_closure;
      "#,
      )
      .await?;
    }

    db.execute_unprepared(
      r#"
      DROP INDEX IF EXISTS idx_users__is_admin;

      DROP TRIGGER IF EXISTS trg_org_closure_lock ON organizations;
      DROP FUNCTION IF EXISTS tg_lock_org_closure();
      DROP TRIGGER IF EXISTS trg_org_closure ON organizations;
      DROP TRIGGER IF EXISTS trg_org_cycle ON organizations;
      DROP FUNCTION IF EXISTS tg_rebuild_org_closure();
      DROP FUNCTION IF EXISTS tg_reject_org_cycle();
      DROP FUNCTION IF EXISTS rebuild_org_closure();
      DROP TABLE IF EXISTS org_closure;
      DROP TABLE IF EXISTS relation_tuples;
      DROP TABLE IF EXISTS permission_definitions;
      DROP TABLE IF EXISTS relation_definitions;
      "#,
    )
    .await?;

    Ok(())
  }
}
