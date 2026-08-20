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
      #[cfg(feature = "system-settings")]
      Box::new(entity::migration::system_settings_category::Migration),
      Box::new(entity::migration::file_system::Migration),
      Box::new(entity::migration::board_system::Migration),
      Box::new(entity::migration::membership::Migration),
      // ReBAC data layer. After board_system — board_group_closure references board_groups.
      Box::new(entity::migration::rebac::Migration),
      // The closure rebuild takes its lock before the write rather than after it. After `rebac` —
      // it replaces the trigger order that migration installed.
      Box::new(entity::migration::rebac_closure_lock::Migration),
      // Additive columns the compiled rule set reads (boards.organization_id, idx_users__is_admin).
      Box::new(entity::migration::rebac_columns::Migration),
      // Translates whatever access_policies rows the migrations above have seeded.
      Box::new(entity::migration::rebac_backfill::Migration),
      // Last: the mirror keeps the two stores in step for every policy written after the backfill.
      Box::new(entity::migration::rebac_mirror::Migration),
      // The index the sign-in lockout reads the log by. It references nothing above it.
      Box::new(entity::migration::access_log_account_index::Migration),
      // The api key holder constraint. After rebac — the orphan sweep clears the grants those keys
      // still hold before the keys themselves go.
      Box::new(entity::migration::api_key_holder_fk::Migration),
    ]
  }
}
