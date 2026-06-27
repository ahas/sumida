pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(entity::migration::init::Migration), //
      Box::new(entity::migration::file_system::Migration),
      Box::new(entity::migration::board_system::Migration),
      Box::new(entity::migration::api_key_hash::Migration),
      Box::new(entity::migration::membership::Migration),
      Box::new(entity::migration::membership_scheduler::Migration),
      Box::new(entity::migration::membership_billing::Migration),
      Box::new(entity::migration::membership_reservations::Migration),
      Box::new(entity::migration::membership_usage::Migration),
      Box::new(entity::migration::membership_trial::Migration),
      Box::new(entity::migration::membership_dunning::Migration),
      Box::new(entity::migration::membership_coupons::Migration),
    ]
  }
}
