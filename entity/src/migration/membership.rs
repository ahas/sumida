use ForeignKeyAction::*;
use sea_orm::Statement;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;
use serde_json::json;

use super::helper::*;

/// membership_plans
#[derive(DeriveIden)]
enum MembershipPlans {
  Table,
  Id,
  Code,
  Name,
  Description,
  Kind,
  Scope,
  Price,
  Currency,
  BillingInterval,
  DurationDays,
  CreditAmount,
  IsEnabled,
  ProviderProductId,
  DeletedAt,
  CreatedAt,
  UpdatedAt,
}

/// plan_actions
#[derive(DeriveIden)]
enum PlanActions {
  Table,
  Id,
  PlanId,
  Action,
  CreditCost,
  QuotaPerPeriod,
}

/// subscriptions
#[derive(DeriveIden)]
enum Subscriptions {
  Table,
  Id,
  Code,
  PlanId,
  PlanCode,
  OwnerKind,
  OwnerId,
  Status,
  CurrentPeriodStart,
  CurrentPeriodEnd,
  CancelAtPeriodEnd,
  CanceledAt,
  AllowedActions,
  IdempotencyKey,
  ProviderSubscriptionId,
  TrialEndsAt,
  GraceEndsAt,
  PrevPlanId,
  CreatedAt,
  UpdatedAt,
}

/// subscription_events
#[derive(DeriveIden)]
enum SubscriptionEvents {
  Table,
  Id,
  SubscriptionId,
  EventKind,
  OldStatus,
  NewStatus,
  Reason,
  TriggeredByUserId,
  ReferenceCode,
  IdempotencyKey,
  CreatedAt,
}

/// membership_purchases
#[derive(DeriveIden)]
enum MembershipPurchases {
  Table,
  Id,
  Code,
  Kind,
  PlanId,
  PlanCode,
  OwnerKind,
  OwnerId,
  Status,
  Amount,
  Currency,
  Provider,
  ProviderPaymentId,
  IdempotencyKey,
  PaidAt,
  RefundedAt,
  Metadata,
  CreatedAt,
  UpdatedAt,
}

/// payment_webhook_events
#[derive(DeriveIden)]
enum PaymentWebhookEvents {
  Table,
  Id,
  Provider,
  WebhookId,
  EventType,
  PaymentId,
  ProcessedAt,
  CreatedAt,
}

/// credit_wallets
#[derive(DeriveIden)]
enum CreditWallets {
  Table,
  Id,
  OwnerKind,
  OwnerId,
  Currency,
  Balance,
  CreatedAt,
  UpdatedAt,
}

/// credit_grants
#[derive(DeriveIden)]
enum CreditGrants {
  Table,
  Id,
  WalletId,
  SourcePlanCode,
  AmountGranted,
  AmountRemaining,
  ExpiryAt,
  ReferenceCode,
  CreatedAt,
}

/// credit_transactions
#[derive(DeriveIden)]
enum CreditTransactions {
  Table,
  Id,
  WalletId,
  GrantId,
  SubscriptionId,
  Kind,
  Amount,
  Action,
  ReferenceCode,
  IdempotencyKey,
  BalanceAfter,
  CreatedAt,
}

/// mail_forms (existing) — used only to seed default templates.
#[derive(DeriveIden)]
enum MailForms {
  Table,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    let backend = db.get_database_backend();

    // ---- membership_plans ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(MembershipPlans::Table)
          .col(integer(MembershipPlans::Id).auto_increment())
          .col(string(MembershipPlans::Code))
          .col(string(MembershipPlans::Name))
          .col(string_null(MembershipPlans::Description))
          .col(string(MembershipPlans::Kind))
          .col(string(MembershipPlans::Scope))
          .col(big_integer(MembershipPlans::Price).default(0))
          .col(string(MembershipPlans::Currency).default("KRW"))
          .col(string_null(MembershipPlans::BillingInterval))
          .col(integer_null(MembershipPlans::DurationDays))
          .col(big_integer_null(MembershipPlans::CreditAmount))
          .col(boolean(MembershipPlans::IsEnabled).default(true))
          .col(string_null(MembershipPlans::ProviderProductId))
          .col(timestamp_with_time_zone_null(MembershipPlans::DeletedAt))
          .col(timestamp_with_time_zone(MembershipPlans::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(MembershipPlans::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_membership_plans__id").col(MembershipPlans::Id))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_membership_plans__code")
          .table(MembershipPlans::Table)
          .col(MembershipPlans::Code)
          .unique()
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "membership_plans").await?;

    // ---- plan_actions ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(PlanActions::Table)
          .col(integer(PlanActions::Id).auto_increment())
          .col(integer(PlanActions::PlanId))
          .col(string(PlanActions::Action))
          .col(big_integer_null(PlanActions::CreditCost))
          .col(integer_null(PlanActions::QuotaPerPeriod))
          .primary_key(Index::create().name("pk_plan_actions__id").col(PlanActions::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_plan_actions__plan_id")
              .from(PlanActions::Table, PlanActions::PlanId)
              .to(MembershipPlans::Table, MembershipPlans::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_plan_actions__plan_id__action")
          .table(PlanActions::Table)
          .col(PlanActions::PlanId)
          .col(PlanActions::Action)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- subscriptions ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Subscriptions::Table)
          .col(integer(Subscriptions::Id).auto_increment())
          .col(string(Subscriptions::Code))
          .col(integer(Subscriptions::PlanId))
          .col(string(Subscriptions::PlanCode))
          .col(string(Subscriptions::OwnerKind))
          .col(integer(Subscriptions::OwnerId))
          .col(string(Subscriptions::Status).default("pending"))
          .col(timestamp_with_time_zone(Subscriptions::CurrentPeriodStart))
          .col(timestamp_with_time_zone(Subscriptions::CurrentPeriodEnd))
          .col(boolean(Subscriptions::CancelAtPeriodEnd).default(false))
          .col(timestamp_with_time_zone_null(Subscriptions::CanceledAt))
          .col(json_binary(Subscriptions::AllowedActions).default(json!([])))
          .col(string_null(Subscriptions::IdempotencyKey))
          .col(string_null(Subscriptions::ProviderSubscriptionId))
          .col(timestamp_with_time_zone_null(Subscriptions::TrialEndsAt))
          .col(timestamp_with_time_zone_null(Subscriptions::GraceEndsAt))
          .col(integer_null(Subscriptions::PrevPlanId))
          .col(timestamp_with_time_zone(Subscriptions::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Subscriptions::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_subscriptions__id").col(Subscriptions::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_subscriptions__plan_id")
              .from(Subscriptions::Table, Subscriptions::PlanId)
              .to(MembershipPlans::Table, MembershipPlans::Id)
              .on_delete(Restrict)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_subscriptions__code")
          .table(Subscriptions::Table)
          .col(Subscriptions::Code)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_subscriptions__idempotency_key")
          .table(Subscriptions::Table)
          .col(Subscriptions::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_subscriptions__owner__status")
          .table(Subscriptions::Table)
          .col(Subscriptions::OwnerKind)
          .col(Subscriptions::OwnerId)
          .col(Subscriptions::Status)
          .to_owned(),
      )
      .await?;
    // Partial unique: at most one ACTIVE subscription per (owner, plan).
    // sea-query has no partial-index builder, so use raw SQL.
    db.execute(Statement::from_string(
      backend,
      r#"CREATE UNIQUE INDEX IF NOT EXISTS "uq_subscriptions__active_owner_plan"
         ON subscriptions (owner_kind, owner_id, plan_id)
         WHERE status = 'active'"#,
    ))
    .await?;
    attach_timestamp_auto_update(db, "subscriptions").await?;

    // ---- subscription_events ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(SubscriptionEvents::Table)
          .col(integer(SubscriptionEvents::Id).auto_increment())
          .col(integer(SubscriptionEvents::SubscriptionId))
          .col(string(SubscriptionEvents::EventKind))
          .col(string_null(SubscriptionEvents::OldStatus))
          .col(string_null(SubscriptionEvents::NewStatus))
          .col(string_null(SubscriptionEvents::Reason))
          .col(integer_null(SubscriptionEvents::TriggeredByUserId))
          .col(string_null(SubscriptionEvents::ReferenceCode))
          .col(string_null(SubscriptionEvents::IdempotencyKey))
          .col(timestamp_with_time_zone(SubscriptionEvents::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_subscription_events__id").col(SubscriptionEvents::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_subscription_events__subscription_id")
              .from(SubscriptionEvents::Table, SubscriptionEvents::SubscriptionId)
              .to(Subscriptions::Table, Subscriptions::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_subscription_events__subscription_id__created_at")
          .table(SubscriptionEvents::Table)
          .col(SubscriptionEvents::SubscriptionId)
          .col(SubscriptionEvents::CreatedAt)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_subscription_events__idempotency_key")
          .table(SubscriptionEvents::Table)
          .col(SubscriptionEvents::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- membership_purchases ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(MembershipPurchases::Table)
          .col(integer(MembershipPurchases::Id).auto_increment())
          .col(string(MembershipPurchases::Code))
          .col(string(MembershipPurchases::Kind))
          .col(integer(MembershipPurchases::PlanId))
          .col(string(MembershipPurchases::PlanCode))
          .col(string(MembershipPurchases::OwnerKind))
          .col(integer(MembershipPurchases::OwnerId))
          .col(string(MembershipPurchases::Status).default("pending"))
          .col(big_integer(MembershipPurchases::Amount).default(0))
          .col(string(MembershipPurchases::Currency).default("KRW"))
          .col(string(MembershipPurchases::Provider).default("none"))
          .col(string_null(MembershipPurchases::ProviderPaymentId))
          .col(string_null(MembershipPurchases::IdempotencyKey))
          .col(timestamp_with_time_zone_null(MembershipPurchases::PaidAt))
          .col(timestamp_with_time_zone_null(MembershipPurchases::RefundedAt))
          .col(json_binary_null(MembershipPurchases::Metadata))
          .col(timestamp_with_time_zone(MembershipPurchases::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(MembershipPurchases::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_membership_purchases__id").col(MembershipPurchases::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_membership_purchases__plan_id")
              .from(MembershipPurchases::Table, MembershipPurchases::PlanId)
              .to(MembershipPlans::Table, MembershipPlans::Id)
              .on_delete(Restrict)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_membership_purchases__code")
          .table(MembershipPurchases::Table)
          .col(MembershipPurchases::Code)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_membership_purchases__idempotency_key")
          .table(MembershipPurchases::Table)
          .col(MembershipPurchases::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_membership_purchases__owner")
          .table(MembershipPurchases::Table)
          .col(MembershipPurchases::OwnerKind)
          .col(MembershipPurchases::OwnerId)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "membership_purchases").await?;

    // ---- payment_webhook_events (idempotent webhook processing) ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(PaymentWebhookEvents::Table)
          .col(integer(PaymentWebhookEvents::Id).auto_increment())
          .col(string(PaymentWebhookEvents::Provider))
          .col(string(PaymentWebhookEvents::WebhookId))
          .col(string(PaymentWebhookEvents::EventType))
          .col(string(PaymentWebhookEvents::PaymentId))
          .col(timestamp_with_time_zone_null(PaymentWebhookEvents::ProcessedAt))
          .col(timestamp_with_time_zone(PaymentWebhookEvents::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_payment_webhook_events__id").col(PaymentWebhookEvents::Id))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_payment_webhook_events__webhook_id")
          .table(PaymentWebhookEvents::Table)
          .col(PaymentWebhookEvents::WebhookId)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- credit_wallets ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CreditWallets::Table)
          .col(integer(CreditWallets::Id).auto_increment())
          .col(string(CreditWallets::OwnerKind))
          .col(integer(CreditWallets::OwnerId))
          .col(string(CreditWallets::Currency).default("KRW"))
          .col(big_integer(CreditWallets::Balance).default(0))
          .col(timestamp_with_time_zone(CreditWallets::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(CreditWallets::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_credit_wallets__id").col(CreditWallets::Id))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_credit_wallets__owner")
          .table(CreditWallets::Table)
          .col(CreditWallets::OwnerKind)
          .col(CreditWallets::OwnerId)
          .unique()
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "credit_wallets").await?;

    // ---- credit_grants ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CreditGrants::Table)
          .col(integer(CreditGrants::Id).auto_increment())
          .col(integer(CreditGrants::WalletId))
          .col(string_null(CreditGrants::SourcePlanCode))
          .col(big_integer(CreditGrants::AmountGranted))
          .col(big_integer(CreditGrants::AmountRemaining))
          .col(timestamp_with_time_zone_null(CreditGrants::ExpiryAt))
          .col(string_null(CreditGrants::ReferenceCode))
          .col(timestamp_with_time_zone(CreditGrants::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_credit_grants__id").col(CreditGrants::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_credit_grants__wallet_id")
              .from(CreditGrants::Table, CreditGrants::WalletId)
              .to(CreditWallets::Table, CreditWallets::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // FIFO draw order: (expiry_at NULLS LAST, created_at) per wallet.
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_credit_grants__wallet__expiry__created")
          .table(CreditGrants::Table)
          .col(CreditGrants::WalletId)
          .col(CreditGrants::ExpiryAt)
          .col(CreditGrants::CreatedAt)
          .to_owned(),
      )
      .await?;

    // ---- credit_transactions ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CreditTransactions::Table)
          .col(big_integer(CreditTransactions::Id).auto_increment())
          .col(integer(CreditTransactions::WalletId))
          .col(integer_null(CreditTransactions::GrantId))
          .col(integer_null(CreditTransactions::SubscriptionId))
          .col(string(CreditTransactions::Kind))
          .col(big_integer(CreditTransactions::Amount))
          .col(string_null(CreditTransactions::Action))
          .col(string_null(CreditTransactions::ReferenceCode))
          .col(string_null(CreditTransactions::IdempotencyKey))
          .col(big_integer(CreditTransactions::BalanceAfter))
          .col(timestamp_with_time_zone(CreditTransactions::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_credit_transactions__id").col(CreditTransactions::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_credit_transactions__wallet_id")
              .from(CreditTransactions::Table, CreditTransactions::WalletId)
              .to(CreditWallets::Table, CreditWallets::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_credit_transactions__grant_id")
              .from(CreditTransactions::Table, CreditTransactions::GrantId)
              .to(CreditGrants::Table, CreditGrants::Id)
              .on_delete(SetNull)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_credit_transactions__wallet__created")
          .table(CreditTransactions::Table)
          .col(CreditTransactions::WalletId)
          .col(CreditTransactions::CreatedAt)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("uq_credit_transactions__wallet__idempotency_key")
          .table(CreditTransactions::Table)
          .col(CreditTransactions::WalletId)
          .col(CreditTransactions::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- seed default mail templates (apps may override the sender/subject/body) ----
    let _ = MailForms::Table; // silence unused-variant lint
    db.execute(Statement::from_string(
      backend,
      r#"INSERT INTO mail_forms (kind, sender, subject, content) VALUES
        ('subscription-created-receipt', 'no-reply@example.com', 'Subscription activated', 'Your subscription to {plan_code} is now active.'),
        ('subscription-expiring-soon', 'no-reply@example.com', 'Subscription expiring soon', 'Your subscription {plan_code} expires at {current_period_end}.'),
        ('credit-purchased-receipt', 'no-reply@example.com', 'Credits added', '{amount} credits were added to your wallet.'),
        ('credit-low-balance-alert', 'no-reply@example.com', 'Low credit balance', 'Your credit balance is low: {balance} remaining.')
      ON CONFLICT (kind) DO NOTHING"#,
    ))
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    drop_table(manager, CreditTransactions::Table).await?;
    drop_table(manager, CreditGrants::Table).await?;
    drop_table(manager, CreditWallets::Table).await?;
    drop_table(manager, PaymentWebhookEvents::Table).await?;
    drop_table(manager, MembershipPurchases::Table).await?;
    drop_table(manager, SubscriptionEvents::Table).await?;
    drop_table(manager, Subscriptions::Table).await?;
    drop_table(manager, PlanActions::Table).await?;
    drop_table(manager, MembershipPlans::Table).await?;
    Ok(())
  }
}
