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
      // The closure rebuild takes its lock before the write rather than after it. After `rebac` —
      // it replaces the trigger order that migration installed.
      Box::new(sumida::entity::migration::rebac_closure_lock::Migration),
      // Additive columns the compiled rule set reads (boards.organization_id, idx_users__is_admin).
      Box::new(sumida::entity::migration::rebac_columns::Migration),
      // Refuses to go on if access_policies still holds a grant nothing here can read.
      Box::new(sumida::entity::migration::rebac_backfill::Migration),
      // Takes the mirror trigger and its functions off that table.
      Box::new(sumida::entity::migration::rebac_mirror::Migration),
      // The moment an account last reset its credentials. An additive column on users.
      Box::new(sumida::entity::migration::user_credentials_reset_at::Migration),
      // The api key holder constraint. After rebac — the orphan sweep clears the grants those keys
      // still hold before the keys themselves go.
      Box::new(sumida::entity::migration::api_key_holder_fk::Migration),
      // The table nothing reads. Last, so the two tombstones above have found it empty first.
      Box::new(sumida::entity::migration::access_policies_drop::Migration),
    ]
  }
}
