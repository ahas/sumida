pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(entity::migration::init::Migration), //
      // Shared operational settings. After init — the row that records who last edited a setting
      // points at users.
      #[cfg(feature = "system-settings")]
      Box::new(entity::migration::system_settings::Migration),
      Box::new(entity::migration::file_system::Migration),
      Box::new(entity::migration::board_system::Migration),
      Box::new(entity::migration::membership::Migration),
      // ReBAC data layer. After board_system — board_group_closure references board_groups, and
      // the column the board rule reads sits on boards.
      Box::new(entity::migration::rebac::Migration),
      // The sign-in log's identifier and the indexes its readers seek by. It references nothing
      // above it.
      Box::new(entity::migration::access_log::Migration),
      // An account's removal takes its sign-in log with it. Only needs `init`.
      Box::new(entity::migration::access_log_cascade::Migration),
    ]
  }
}
