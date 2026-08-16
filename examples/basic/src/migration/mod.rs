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
      // Last: it translates whatever access_policies rows the migrations above have seeded.
      Box::new(sumida::entity::migration::rebac_backfill::Migration),
    ]
  }
}
