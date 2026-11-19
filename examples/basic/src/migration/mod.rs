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
      // ReBAC data layer. After board_system — board_group_closure references board_groups, and
      // the column the board rule reads sits on boards.
      Box::new(sumida::entity::migration::rebac::Migration),
      // An account's removal takes its sign-in log with it. Only needs init.
      Box::new(sumida::entity::migration::access_log_cascade::Migration),
      // Supersedes the cascade for the sign-in log: the rows stay and the account column goes
      // nullable. Only needs init.
      Box::new(sumida::entity::migration::access_log_set_null::Migration),
      // Debt, amount quotas and quota holds. Alters and references the membership tables.
      Box::new(sumida::entity::migration::membership_metering::Migration),
    ]
  }
}
