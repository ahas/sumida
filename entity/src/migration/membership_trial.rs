use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

/// membership_plans (existing) — add the trial length.
#[derive(DeriveIden)]
enum MembershipPlans {
  Table,
  TrialDays,
}

/// Additive follow-up to `membership`: a plan-level free-trial length. When set,
/// a new subscription's first period runs free for `trial_days` (with
/// `trial_ends_at` marked); the scheduler then auto-renews or expires it.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(MembershipPlans::Table)
          .add_column_if_not_exists(integer_null(MembershipPlans::TrialDays))
          .to_owned(),
      )
      .await?;
    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
      .alter_table(
        Table::alter()
          .table(MembershipPlans::Table)
          .drop_column(MembershipPlans::TrialDays)
          .to_owned(),
      )
      .await?;
    Ok(())
  }
}
