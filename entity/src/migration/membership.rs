use ForeignKeyAction::*;
use sea_orm::Statement;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;
use serde_json::json;

use super::helper::*;

/// Single migration for the whole membership/billing domain (subscriptions,
/// credits + reservations, entitlement usage, payments, coupons). Kept as one
/// migration intentionally — the feature is unreleased, so there is no value in a
/// chain of incremental ALTERs; this is the authoritative final schema.

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
  TrialDays,
  CreditAmount,
  IsEnabled,
  ProviderProductId,
  DeletedAt,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum PlanActions {
  Table,
  Id,
  PlanId,
  Action,
  CreditCost,
  QuotaPerPeriod,
}

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
  AutoRenew,
  BillingKey,
  DunningAttempts,
  ProviderSubscriptionId,
  TrialEndsAt,
  GraceEndsAt,
  PrevPlanId,
  ExpiringNotifiedAt,
  CreatedAt,
  UpdatedAt,
}

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

#[derive(DeriveIden)]
enum SubscriptionUsage {
  Table,
  Id,
  SubscriptionId,
  Action,
  PeriodStart,
  Used,
  CreatedAt,
  UpdatedAt,
}

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
  Subtotal,
  DiscountAmount,
  TaxAmount,
  CouponCode,
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

#[derive(DeriveIden)]
enum CreditWallets {
  Table,
  Id,
  OwnerKind,
  OwnerId,
  Balance,
  LowBalanceNotifiedAt,
  CreatedAt,
  UpdatedAt,
}

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

#[derive(DeriveIden)]
enum CreditReservations {
  Table,
  Id,
  WalletId,
  Amount,
  Status,
  Action,
  ReferenceCode,
  IdempotencyKey,
  ExpiresAt,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum Coupons {
  Table,
  Id,
  Code,
  PercentOff,
  GrantCredits,
  FreePlanCode,
  Currency,
  MaxRedemptions,
  MaxRedemptionsPerOwner,
  RedeemedCount,
  RequiresGrant,
  Stackable,
  ExpiresAt,
  IsActive,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum CouponPlans {
  Table,
  Id,
  CouponId,
  PlanCode,
}

#[derive(DeriveIden)]
enum CouponGrants {
  Table,
  Id,
  CouponId,
  OwnerKind,
  OwnerId,
  RedeemedAt,
  RevokedAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum CouponRedemptions {
  Table,
  Id,
  CouponId,
  OwnerKind,
  OwnerId,
  PurchaseCode,
  GrantId,
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
          .col(integer_null(MembershipPlans::TrialDays))
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
        idx("uq_membership_plans__code", MembershipPlans::Table)
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
          .foreign_key(&mut fk(
            "fk_plan_actions__plan_id",
            PlanActions::Table,
            PlanActions::PlanId,
            MembershipPlans::Table,
            MembershipPlans::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_plan_actions__plan_id__action", PlanActions::Table)
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
          .col(json_binary(Subscriptions::AllowedActions).default(json!({})))
          .col(string_null(Subscriptions::IdempotencyKey))
          .col(boolean(Subscriptions::AutoRenew).default(false))
          .col(string_null(Subscriptions::BillingKey))
          .col(integer(Subscriptions::DunningAttempts).default(0))
          .col(string_null(Subscriptions::ProviderSubscriptionId))
          .col(timestamp_with_time_zone_null(Subscriptions::TrialEndsAt))
          .col(timestamp_with_time_zone_null(Subscriptions::GraceEndsAt))
          .col(integer_null(Subscriptions::PrevPlanId))
          .col(timestamp_with_time_zone_null(Subscriptions::ExpiringNotifiedAt))
          .col(timestamp_with_time_zone(Subscriptions::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Subscriptions::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_subscriptions__id").col(Subscriptions::Id))
          .foreign_key(&mut fk(
            "fk_subscriptions__plan_id",
            Subscriptions::Table,
            Subscriptions::PlanId,
            MembershipPlans::Table,
            MembershipPlans::Id,
            Restrict,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_subscriptions__code", Subscriptions::Table)
          .col(Subscriptions::Code)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_subscriptions__idempotency_key", Subscriptions::Table)
          .col(Subscriptions::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("idx_subscriptions__owner__status", Subscriptions::Table)
          .col(Subscriptions::OwnerKind)
          .col(Subscriptions::OwnerId)
          .col(Subscriptions::Status)
          .to_owned(),
      )
      .await?;
    db.execute(Statement::from_string(
      backend,
      r#"CREATE UNIQUE INDEX IF NOT EXISTS "uq_subscriptions__active_owner_plan"
         ON subscriptions (owner_kind, owner_id, plan_id) WHERE status = 'active'"#,
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
          .foreign_key(&mut fk(
            "fk_subscription_events__subscription_id",
            SubscriptionEvents::Table,
            SubscriptionEvents::SubscriptionId,
            Subscriptions::Table,
            Subscriptions::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("idx_subscription_events__subscription_id__created_at", SubscriptionEvents::Table)
          .col(SubscriptionEvents::SubscriptionId)
          .col(SubscriptionEvents::CreatedAt)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_subscription_events__idempotency_key", SubscriptionEvents::Table)
          .col(SubscriptionEvents::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- subscription_usage ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(SubscriptionUsage::Table)
          .col(big_integer(SubscriptionUsage::Id).auto_increment())
          .col(integer(SubscriptionUsage::SubscriptionId))
          .col(string(SubscriptionUsage::Action))
          .col(timestamp_with_time_zone(SubscriptionUsage::PeriodStart))
          .col(big_integer(SubscriptionUsage::Used).default(0))
          .col(timestamp_with_time_zone(SubscriptionUsage::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(SubscriptionUsage::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_subscription_usage__id").col(SubscriptionUsage::Id))
          .foreign_key(&mut fk(
            "fk_subscription_usage__subscription_id",
            SubscriptionUsage::Table,
            SubscriptionUsage::SubscriptionId,
            Subscriptions::Table,
            Subscriptions::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_subscription_usage__sub__action__period", SubscriptionUsage::Table)
          .col(SubscriptionUsage::SubscriptionId)
          .col(SubscriptionUsage::Action)
          .col(SubscriptionUsage::PeriodStart)
          .unique()
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "subscription_usage").await?;

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
          .col(big_integer(MembershipPurchases::Subtotal).default(0))
          .col(big_integer(MembershipPurchases::DiscountAmount).default(0))
          .col(big_integer(MembershipPurchases::TaxAmount).default(0))
          .col(string_null(MembershipPurchases::CouponCode))
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
          .foreign_key(&mut fk(
            "fk_membership_purchases__plan_id",
            MembershipPurchases::Table,
            MembershipPurchases::PlanId,
            MembershipPlans::Table,
            MembershipPlans::Id,
            Restrict,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_membership_purchases__code", MembershipPurchases::Table)
          .col(MembershipPurchases::Code)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_membership_purchases__idempotency_key", MembershipPurchases::Table)
          .col(MembershipPurchases::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("idx_membership_purchases__owner", MembershipPurchases::Table)
          .col(MembershipPurchases::OwnerKind)
          .col(MembershipPurchases::OwnerId)
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "membership_purchases").await?;

    // ---- payment_webhook_events ----
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
        idx("uq_payment_webhook_events__webhook_id", PaymentWebhookEvents::Table)
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
          .col(big_integer(CreditWallets::Balance).default(0))
          .col(timestamp_with_time_zone_null(CreditWallets::LowBalanceNotifiedAt))
          .col(timestamp_with_time_zone(CreditWallets::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(CreditWallets::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_credit_wallets__id").col(CreditWallets::Id))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_credit_wallets__owner", CreditWallets::Table)
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
          .foreign_key(&mut fk(
            "fk_credit_grants__wallet_id",
            CreditGrants::Table,
            CreditGrants::WalletId,
            CreditWallets::Table,
            CreditWallets::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("idx_credit_grants__wallet__expiry__created", CreditGrants::Table)
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
          .foreign_key(&mut fk(
            "fk_credit_transactions__wallet_id",
            CreditTransactions::Table,
            CreditTransactions::WalletId,
            CreditWallets::Table,
            CreditWallets::Id,
            Cascade,
          ))
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
        idx("idx_credit_transactions__wallet__created", CreditTransactions::Table)
          .col(CreditTransactions::WalletId)
          .col(CreditTransactions::CreatedAt)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_credit_transactions__wallet__idempotency_key", CreditTransactions::Table)
          .col(CreditTransactions::WalletId)
          .col(CreditTransactions::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- credit_reservations ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CreditReservations::Table)
          .col(integer(CreditReservations::Id).auto_increment())
          .col(integer(CreditReservations::WalletId))
          .col(big_integer(CreditReservations::Amount))
          .col(string(CreditReservations::Status).default("held"))
          .col(string_null(CreditReservations::Action))
          .col(string_null(CreditReservations::ReferenceCode))
          .col(string_null(CreditReservations::IdempotencyKey))
          .col(timestamp_with_time_zone_null(CreditReservations::ExpiresAt))
          .col(timestamp_with_time_zone(CreditReservations::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(CreditReservations::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_credit_reservations__id").col(CreditReservations::Id))
          .foreign_key(&mut fk(
            "fk_credit_reservations__wallet_id",
            CreditReservations::Table,
            CreditReservations::WalletId,
            CreditWallets::Table,
            CreditWallets::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("idx_credit_reservations__wallet__status", CreditReservations::Table)
          .col(CreditReservations::WalletId)
          .col(CreditReservations::Status)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_credit_reservations__wallet__idempotency_key", CreditReservations::Table)
          .col(CreditReservations::WalletId)
          .col(CreditReservations::IdempotencyKey)
          .unique()
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "credit_reservations").await?;

    // ---- coupons ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Coupons::Table)
          .col(integer(Coupons::Id).auto_increment())
          .col(string(Coupons::Code))
          .col(integer_null(Coupons::PercentOff))
          .col(big_integer_null(Coupons::GrantCredits))
          .col(string_null(Coupons::FreePlanCode))
          .col(string_null(Coupons::Currency))
          .col(integer_null(Coupons::MaxRedemptions))
          .col(integer_null(Coupons::MaxRedemptionsPerOwner))
          .col(integer(Coupons::RedeemedCount).default(0))
          .col(boolean(Coupons::RequiresGrant).default(false))
          .col(boolean(Coupons::Stackable).default(false))
          .col(timestamp_with_time_zone_null(Coupons::ExpiresAt))
          .col(boolean(Coupons::IsActive).default(true))
          .col(timestamp_with_time_zone(Coupons::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Coupons::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_coupons__id").col(Coupons::Id))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(idx("uq_coupons__code", Coupons::Table).col(Coupons::Code).unique().to_owned())
      .await?;
    attach_timestamp_auto_update(db, "coupons").await?;

    // ---- coupon_plans ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CouponPlans::Table)
          .col(integer(CouponPlans::Id).auto_increment())
          .col(integer(CouponPlans::CouponId))
          .col(string(CouponPlans::PlanCode))
          .primary_key(Index::create().name("pk_coupon_plans__id").col(CouponPlans::Id))
          .foreign_key(&mut fk(
            "fk_coupon_plans__coupon_id",
            CouponPlans::Table,
            CouponPlans::CouponId,
            Coupons::Table,
            Coupons::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_coupon_plans__coupon__plan", CouponPlans::Table)
          .col(CouponPlans::CouponId)
          .col(CouponPlans::PlanCode)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- coupon_grants ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CouponGrants::Table)
          .col(integer(CouponGrants::Id).auto_increment())
          .col(integer(CouponGrants::CouponId))
          .col(string(CouponGrants::OwnerKind))
          .col(integer(CouponGrants::OwnerId))
          .col(timestamp_with_time_zone_null(CouponGrants::RedeemedAt))
          .col(timestamp_with_time_zone_null(CouponGrants::RevokedAt))
          .col(timestamp_with_time_zone(CouponGrants::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_coupon_grants__id").col(CouponGrants::Id))
          .foreign_key(&mut fk(
            "fk_coupon_grants__coupon_id",
            CouponGrants::Table,
            CouponGrants::CouponId,
            Coupons::Table,
            Coupons::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("uq_coupon_grants__coupon__owner", CouponGrants::Table)
          .col(CouponGrants::CouponId)
          .col(CouponGrants::OwnerKind)
          .col(CouponGrants::OwnerId)
          .unique()
          .to_owned(),
      )
      .await?;

    // ---- coupon_redemptions ----
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(CouponRedemptions::Table)
          .col(integer(CouponRedemptions::Id).auto_increment())
          .col(integer(CouponRedemptions::CouponId))
          .col(string(CouponRedemptions::OwnerKind))
          .col(integer(CouponRedemptions::OwnerId))
          .col(string_null(CouponRedemptions::PurchaseCode))
          .col(integer_null(CouponRedemptions::GrantId))
          .col(timestamp_with_time_zone(CouponRedemptions::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_coupon_redemptions__id").col(CouponRedemptions::Id))
          .foreign_key(&mut fk(
            "fk_coupon_redemptions__coupon_id",
            CouponRedemptions::Table,
            CouponRedemptions::CouponId,
            Coupons::Table,
            Coupons::Id,
            Cascade,
          ))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        idx("idx_coupon_redemptions__coupon__owner", CouponRedemptions::Table)
          .col(CouponRedemptions::CouponId)
          .col(CouponRedemptions::OwnerKind)
          .col(CouponRedemptions::OwnerId)
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
    drop_table(manager, CouponRedemptions::Table).await?;
    drop_table(manager, CouponGrants::Table).await?;
    drop_table(manager, CouponPlans::Table).await?;
    drop_table(manager, Coupons::Table).await?;
    drop_table(manager, CreditReservations::Table).await?;
    drop_table(manager, CreditTransactions::Table).await?;
    drop_table(manager, CreditGrants::Table).await?;
    drop_table(manager, CreditWallets::Table).await?;
    drop_table(manager, PaymentWebhookEvents::Table).await?;
    drop_table(manager, MembershipPurchases::Table).await?;
    drop_table(manager, SubscriptionUsage::Table).await?;
    drop_table(manager, SubscriptionEvents::Table).await?;
    drop_table(manager, Subscriptions::Table).await?;
    drop_table(manager, PlanActions::Table).await?;
    drop_table(manager, MembershipPlans::Table).await?;
    Ok(())
  }
}

/// Index builder shorthand: `create index if not exists <name> on <table>`.
fn idx<T: IntoIden + 'static>(name: &str, table: T) -> IndexCreateStatement {
  Index::create().if_not_exists().name(name).table(table).to_owned()
}

/// Foreign-key shorthand with `on_update = Cascade`.
fn fk<T: IntoIden + 'static, F: IntoIden + 'static, RT: IntoIden + 'static, RC: IntoIden + 'static>(
  name: &str,
  from_tbl: T,
  from_col: F,
  to_tbl: RT,
  to_col: RC,
  on_delete: ForeignKeyAction,
) -> ForeignKeyCreateStatement {
  ForeignKey::create()
    .name(name)
    .from(from_tbl, from_col)
    .to(to_tbl, to_col)
    .on_delete(on_delete)
    .on_update(Cascade)
    .to_owned()
}
