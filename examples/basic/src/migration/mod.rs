pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(sumida::entity::migration::init::Migration),
      Box::new(sumida::entity::migration::file_system::Migration),
      Box::new(sumida::entity::migration::board_system::Migration),
      Box::new(sumida::entity::migration::membership::Migration),
      // ReBAC data layer. After board_system — board_group_closure references board_groups.
      Box::new(sumida::entity::migration::rebac::Migration),
      // Additive columns the compiled rule set reads (boards.organization_id, idx_users__is_admin).
      Box::new(sumida::entity::migration::rebac_columns::Migration),
      // Translates whatever access_policies rows the migrations above have seeded.
      Box::new(sumida::entity::migration::rebac_backfill::Migration),
      // Last: the mirror keeps the two stores in step for every policy written after the backfill.
      Box::new(sumida::entity::migration::rebac_mirror::Migration),
      // The api key holder constraint. After rebac — the orphan sweep clears the grants those keys
      // still hold before the keys themselves go.
      Box::new(sumida::entity::migration::api_key_holder_fk::Migration),
    ]
  }
}
