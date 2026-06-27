pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(sumida::entity::migration::init::Migration),
      Box::new(sumida::entity::migration::file_system::Migration),
      Box::new(sumida::entity::migration::board_system::Migration),
      Box::new(sumida::entity::migration::api_key_hash::Migration),
      Box::new(sumida::entity::migration::membership::Migration),
    ]
  }
}
