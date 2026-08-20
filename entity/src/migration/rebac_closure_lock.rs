use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// Takes the closure-rebuild lock before the write, instead of after it.
///
/// A closure table is rebuilt whole, under an advisory lock that keeps two rebuilds from
/// interleaving their `DELETE` and `INSERT`. That lock is taken inside the rebuild, and the rebuild
/// runs from an `AFTER … FOR EACH STATEMENT` trigger — so by the time a transaction reaches it, it
/// is already holding row locks on the table it wrote. Two transactions writing different rows of
/// the same tree therefore take their locks in the opposite order from each other: each holds a row
/// the other's rebuild has to read for its foreign keys, and each wants the advisory lock the other
/// holds. Postgres breaks the cycle by killing one, and the caller sees a 500 on an ordinary write.
///
/// Taking the same lock in a `BEFORE … FOR EACH STATEMENT` trigger fixes the order for every
/// writer: the advisory lock always comes first, rows always come second, and no two writers can
/// cross. Advisory locks are re-entrant within a transaction, so the rebuild still takes it and
/// still holds it for anything that reaches the rebuild by another route.
///
/// The lock is in the database rather than in the callers because a caller that forgets it reopens
/// the hole silently, and nothing about a write to `organizations` announces that a whole table is
/// about to be rebuilt underneath it.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Whether the board half applies to *this database*. Same runtime probe as the migration that
/// built the closures: features are unified across the dependency graph, so a build can carry the
/// board feature while the database it migrates has no board tables.
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

    db.execute_unprepared(
      r#"
      CREATE OR REPLACE FUNCTION tg_lock_org_closure() RETURNS trigger AS $$
      BEGIN
        PERFORM pg_advisory_xact_lock(hashtext('org_closure'));
        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_org_closure_lock ON organizations;
      CREATE TRIGGER trg_org_closure_lock
        BEFORE INSERT OR DELETE OR UPDATE OF parent_id ON organizations
        FOR EACH STATEMENT EXECUTE FUNCTION tg_lock_org_closure();
      "#,
    )
    .await?;

    if has_board_groups(db).await? {
      db.execute_unprepared(
        r#"
      CREATE OR REPLACE FUNCTION tg_lock_board_group_closure() RETURNS trigger AS $$
      BEGIN
        PERFORM pg_advisory_xact_lock(hashtext('board_group_closure'));
        RETURN NULL;
      END; $$ LANGUAGE plpgsql;

      DROP TRIGGER IF EXISTS trg_board_group_closure_lock ON board_groups;
      CREATE TRIGGER trg_board_group_closure_lock
        BEFORE INSERT OR DELETE OR UPDATE OF parent_id ON board_groups
        FOR EACH STATEMENT EXECUTE FUNCTION tg_lock_board_group_closure();
      "#,
      )
      .await?;
    }

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    if has_board_groups(db).await? {
      db.execute_unprepared(
        r#"
      DROP TRIGGER IF EXISTS trg_board_group_closure_lock ON board_groups;
      DROP FUNCTION IF EXISTS tg_lock_board_group_closure();
      "#,
      )
      .await?;
    }

    db.execute_unprepared(
      r#"
      DROP TRIGGER IF EXISTS trg_org_closure_lock ON organizations;
      DROP FUNCTION IF EXISTS tg_lock_org_closure();
      "#,
    )
    .await?;

    Ok(())
  }
}
