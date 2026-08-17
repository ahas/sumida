pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(entity::migration::init::Migration), //
      Box::new(entity::migration::file_system::Migration),
      Box::new(entity::migration::board_system::Migration),
      Box::new(entity::migration::membership::Migration),
      // ReBAC data layer. After board_system — board_group_closure references board_groups.
      Box::new(entity::migration::rebac::Migration),
      // Additive columns the compiled rule set reads (boards.organization_id, idx_users__is_admin).
      Box::new(entity::migration::rebac_columns::Migration),
      // Translates whatever access_policies rows the migrations above have seeded.
      Box::new(entity::migration::rebac_backfill::Migration),
      // Last: the mirror keeps the two stores in step for every policy written after the backfill.
      Box::new(entity::migration::rebac_mirror::Migration),
    ]
  }
}
