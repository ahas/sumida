use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// Metered spending on top of the membership schema: a wallet's debt and its ledger
/// column, per-period amount quotas on plan actions and subscriptions, the period's
/// amount usage, and the holds and ledger of amount quotas.
///
/// It stands after `membership`, whose tables it alters and references. Every
/// statement is guarded, so a database that already has a column or table keeps it.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      ALTER TABLE credit_wallets ADD COLUMN IF NOT EXISTS debt bigint NOT NULL DEFAULT 0;
      ALTER TABLE credit_transactions ADD COLUMN IF NOT EXISTS debt_change bigint NOT NULL DEFAULT 0;
      ALTER TABLE plan_actions ADD COLUMN IF NOT EXISTS amount_quota_per_period bigint NULL;
      ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS amount_quotas jsonb NOT NULL DEFAULT '{}'::jsonb;
      ALTER TABLE subscription_usage ADD COLUMN IF NOT EXISTS amount_used bigint NOT NULL DEFAULT 0;

      CREATE TABLE IF NOT EXISTS quota_reservations (
        id serial NOT NULL,
        subscription_id integer NOT NULL,
        action varchar NOT NULL,
        period_start timestamptz NOT NULL,
        amount bigint NOT NULL,
        status varchar NOT NULL DEFAULT 'held',
        reference_code varchar NULL,
        idempotency_key varchar NULL,
        expires_at timestamptz NOT NULL,
        created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
        CONSTRAINT pk_quota_reservations__id PRIMARY KEY (id),
        CONSTRAINT fk_quota_reservations__subscription_id FOREIGN KEY (subscription_id)
          REFERENCES subscriptions (id) ON DELETE CASCADE ON UPDATE CASCADE
      );
      CREATE INDEX IF NOT EXISTS idx_quota_reservations__usage__status
        ON quota_reservations (subscription_id, action, period_start, status);
      CREATE UNIQUE INDEX IF NOT EXISTS uq_quota_reservations__sub__idempotency_key
        ON quota_reservations (subscription_id, idempotency_key);
      CREATE INDEX IF NOT EXISTS idx_quota_reservations__status__expires
        ON quota_reservations (status, expires_at);

      CREATE TABLE IF NOT EXISTS quota_entries (
        id bigserial NOT NULL,
        subscription_id integer NOT NULL,
        action varchar NOT NULL,
        period_start timestamptz NOT NULL,
        kind varchar NOT NULL,
        amount_change bigint NOT NULL DEFAULT 0,
        held_change bigint NOT NULL DEFAULT 0,
        reservation_id integer NULL,
        reference_code varchar NULL,
        idempotency_key varchar NULL,
        created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
        CONSTRAINT pk_quota_entries__id PRIMARY KEY (id),
        CONSTRAINT fk_quota_entries__subscription_id FOREIGN KEY (subscription_id)
          REFERENCES subscriptions (id) ON DELETE CASCADE ON UPDATE CASCADE,
        CONSTRAINT fk_quota_entries__reservation_id FOREIGN KEY (reservation_id)
          REFERENCES quota_reservations (id) ON DELETE SET NULL ON UPDATE CASCADE
      );
      CREATE INDEX IF NOT EXISTS idx_quota_entries__usage
        ON quota_entries (subscription_id, action, period_start);
      CREATE UNIQUE INDEX IF NOT EXISTS uq_quota_entries__sub__idempotency_key
        ON quota_entries (subscription_id, idempotency_key);
      "#,
    )
    .await?;

    super::helper::attach_timestamp_auto_update(db, "quota_reservations").await?;
    Ok(())
  }

  /// Takes the quota ledger and holds away and drops the added columns.
  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      DROP TABLE IF EXISTS quota_entries;
      DROP TABLE IF EXISTS quota_reservations;
      ALTER TABLE subscription_usage DROP COLUMN IF EXISTS amount_used;
      ALTER TABLE subscriptions DROP COLUMN IF EXISTS amount_quotas;
      ALTER TABLE plan_actions DROP COLUMN IF EXISTS amount_quota_per_period;
      ALTER TABLE credit_transactions DROP COLUMN IF EXISTS debt_change;
      ALTER TABLE credit_wallets DROP COLUMN IF EXISTS debt;
      "#,
    )
    .await?;
    Ok(())
  }
}
