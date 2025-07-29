pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(entity::migration::init::Migration), //
      Box::new(entity::migration::board_system::Migration),
    ]
  }
}
