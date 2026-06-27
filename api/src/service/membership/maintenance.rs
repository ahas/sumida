//! Background maintenance for the membership domain.
//!
//! All period/expiry checks elsewhere are *lazy* (evaluated when a row is touched),
//! so without this the denormalized `status`, `cancel_at_period_end` effects, cached
//! wallet balances, and the "expiring soon" / "low balance" notices never advance on
//! their own. [`spawn`] runs these jobs on a fixed interval; [`run_once`] runs one
//! pass (handy for tests and one-shot CLIs).
//!
//! Each job is independent and self-contained: a failure in one is logged and does
//! not abort the others, and every mutation is idempotent so re-running is safe.

use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use entity::membership::subscriptions::Column as SubColumn;
use entity::membership::{
  OwnerKind, PlanKind, PurchaseStatus, ReservationStatus, SubscriptionEventKind, SubscriptionStatus,
};
use entity::prelude::*;
use error::SumidaError;
use infra::{payment, settings};
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::Expr;
use sea_orm::{
  ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
  TransactionTrait,
};
use serde_json::json;

use super::credit::CreditService;
use super::{gen_code, notify};
use crate::SumidaState;

/// How many due subscriptions to process per batch (one transaction each).
const BATCH: u64 = 500;

/// Counts of work done by one maintenance pass.
#[derive(Debug, Default, Clone, Copy)]
pub struct MaintenanceReport {
  pub subscriptions_renewed: usize,
  pub subscriptions_renew_failed: usize,
  pub dunning_recovered: usize,
  pub dunning_failed: usize,
  pub subscriptions_expired: usize,
  pub reservations_released: usize,
  pub wallets_swept: usize,
  pub expiring_notices: usize,
  pub low_balance_notices: usize,
}

/// Auto-renew subscriptions whose period has ended: charge the stored billing
/// key via the configured PG (`charge_recurring`) and, on success, extend the
/// period (+ canonical purchase + `renewed` event); on a declined/errored charge
/// move the subscription to `past_due` (+ `status_changed` event). Returns
/// `(renewed, failed)`.
///
/// Runs BEFORE [`expire_due_subscriptions`] in [`run_once`] so a successful renew
/// pre-empts expiry; failures land in `past_due`, which the expire job ignores.
/// The provider call happens while the row lock is held — acceptable for a
/// single-runner background job and it prevents a concurrent double-charge.
pub async fn renew_due_subscriptions(db: &DatabaseConnection) -> Result<(usize, usize), SumidaError> {
  let now: DateTimeWithTimeZone = Utc::now().into();
  let provider = payment::provider();
  let mut renewed = 0usize;
  let mut failed = 0usize;

  loop {
    let due = Subscriptions::find()
      .filter(SubColumn::Status.eq(SubscriptionStatus::Active))
      .filter(SubColumn::CurrentPeriodEnd.lte(now))
      .filter(SubColumn::AutoRenew.eq(true))
      .filter(SubColumn::CancelAtPeriodEnd.eq(false))
      .filter(SubColumn::BillingKey.is_not_null())
      .order_by_asc(SubColumn::Id)
      .limit(BATCH)
      .all(db)
      .await?;
    if due.is_empty() {
      break;
    }
    let batch_len = due.len();

    for sub in due {
      let txn = db.begin().await?;
      let Some(row) = Subscriptions::find_by_id(sub.id).lock_exclusive().one(&txn).await? else {
        txn.rollback().await?;
        continue;
      };
      // Re-check under lock; bail if no longer eligible.
      let still_due = row.status == SubscriptionStatus::Active
        && row.current_period_end <= now
        && row.auto_renew
        && !row.cancel_at_period_end;
      let Some(billing_key) = row.billing_key.clone().filter(|_| still_due) else {
        txn.rollback().await?;
        continue;
      };

      let Some(plan) = MembershipPlans::find_by_id(row.plan_id).one(&txn).await? else {
        txn.rollback().await?;
        continue;
      };
      let duration = plan.duration_days.unwrap_or(30).max(0);
      let order_code = gen_code("rnw");

      let charge = provider
        .charge_recurring(&billing_key, plan.price, &plan.currency, &order_code)
        .await;
      let paid = matches!(&charge, Ok(v) if v.status.is_paid());

      if paid {
        let base = if row.current_period_end > now { row.current_period_end } else { now };
        let new_end = base + Duration::days(duration as i64);

        // New period (advances period_start → resets per-period action quota).
        Subscriptions::update(SubscriptionsActiveModel {
          id: Unchanged(row.id),
          status: Set(SubscriptionStatus::Active),
          current_period_start: Set(base),
          current_period_end: Set(new_end),
          expiring_notified_at: Set(None),
          grace_ends_at: Set(None),
          dunning_attempts: Set(0),
          ..Default::default()
        })
        .exec(&txn)
        .await?;

        let txn_id = charge.ok().and_then(|v| v.transaction_id);
        MembershipPurchases::insert(MembershipPurchasesActiveModel {
          code: Set(order_code.clone()),
          kind: Set(PlanKind::Subscription),
          plan_id: Set(plan.id),
          plan_code: Set(plan.code.clone()),
          owner_kind: Set(row.owner_kind.clone()),
          owner_id: Set(row.owner_id),
          status: Set(PurchaseStatus::Completed),
          amount: Set(plan.price),
          currency: Set(plan.currency.clone()),
          provider: Set(provider.name().to_string()),
          provider_payment_id: Set(txn_id),
          paid_at: Set(Some(now)),
          ..Default::default()
        })
        .exec(&txn)
        .await?;

        SubscriptionEvents::insert(SubscriptionEventsActiveModel {
          subscription_id: Set(row.id),
          event_kind: Set(SubscriptionEventKind::Renewed),
          old_status: Set(Some("active".to_string())),
          new_status: Set(Some("active".to_string())),
          reason: Set(Some("auto-renew".to_string())),
          reference_code: Set(Some(order_code)),
          ..Default::default()
        })
        .exec(&txn)
        .await?;

        txn.commit().await?;
        renewed += 1;
      } else {
        // Open a grace window during which entitlement continues (if configured).
        let grace_days = settings::get().membership.grace_days.max(0);
        let grace_ends_at = (grace_days > 0).then(|| now + Duration::days(grace_days as i64));
        Subscriptions::update(SubscriptionsActiveModel {
          id: Unchanged(row.id),
          status: Set(SubscriptionStatus::PastDue),
          grace_ends_at: Set(grace_ends_at),
          ..Default::default()
        })
        .exec(&txn)
        .await?;

        SubscriptionEvents::insert(SubscriptionEventsActiveModel {
          subscription_id: Set(row.id),
          event_kind: Set(SubscriptionEventKind::StatusChanged),
          old_status: Set(Some("active".to_string())),
          new_status: Set(Some("past-due".to_string())),
          reason: Set(Some("auto-renew-charge-failed".to_string())),
          ..Default::default()
        })
        .exec(&txn)
        .await?;

        txn.commit().await?;
        failed += 1;
      }
    }

    if (batch_len as u64) < BATCH {
      break;
    }
  }

  Ok((renewed, failed))
}

/// Dunning: retry the renewal charge for `past_due` subscriptions still inside
/// their grace window, up to `max_dunning_attempts`. One retry per call (per
/// scheduler tick): a recovery returns the subscription to `active` (new period,
/// counters reset); a failure increments `dunning_attempts` (final expiry is left
/// to [`expire_due_subscriptions`] at grace end). Returns `(recovered, failed)`.
///
/// Disabled when `max_dunning_attempts == 0`.
pub async fn retry_dunning_subscriptions(db: &DatabaseConnection) -> Result<(usize, usize), SumidaError> {
  let max = settings::get().membership.max_dunning_attempts.max(0);
  if max == 0 {
    return Ok((0, 0));
  }
  let now: DateTimeWithTimeZone = Utc::now().into();
  let provider = payment::provider();
  let mut recovered = 0usize;
  let mut failed = 0usize;

  // Single bounded pass — a failed retry stays eligible, so re-querying in a loop
  // would exhaust all attempts in one tick. Remaining rows are handled next tick.
  let due = Subscriptions::find()
    .filter(SubColumn::Status.eq(SubscriptionStatus::PastDue))
    .filter(SubColumn::AutoRenew.eq(true))
    .filter(SubColumn::BillingKey.is_not_null())
    .filter(SubColumn::GraceEndsAt.is_not_null())
    .filter(SubColumn::GraceEndsAt.gt(now))
    .filter(SubColumn::DunningAttempts.lt(max))
    .order_by_asc(SubColumn::Id)
    .limit(BATCH)
    .all(db)
    .await?;

  for sub in due {
    let txn = db.begin().await?;
    let Some(row) = Subscriptions::find_by_id(sub.id).lock_exclusive().one(&txn).await? else {
      txn.rollback().await?;
      continue;
    };
    let eligible = row.status == SubscriptionStatus::PastDue
      && row.auto_renew
      && row.dunning_attempts < max
      && row.grace_ends_at.map(|g| g > now).unwrap_or(false);
    let Some(billing_key) = row.billing_key.clone().filter(|_| eligible) else {
      txn.rollback().await?;
      continue;
    };
    let Some(plan) = MembershipPlans::find_by_id(row.plan_id).one(&txn).await? else {
      txn.rollback().await?;
      continue;
    };

    let order_code = gen_code("dun");
    let charge =
      provider.charge_recurring(&billing_key, plan.price, &plan.currency, &order_code).await;
    let paid = matches!(&charge, Ok(v) if v.status.is_paid());

    if paid {
      let duration = plan.duration_days.unwrap_or(30).max(0);
      let new_end = now + Duration::days(duration as i64);
      Subscriptions::update(SubscriptionsActiveModel {
        id: Unchanged(row.id),
        status: Set(SubscriptionStatus::Active),
        current_period_start: Set(now),
        current_period_end: Set(new_end),
        grace_ends_at: Set(None),
        dunning_attempts: Set(0),
        expiring_notified_at: Set(None),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      let txn_id = charge.ok().and_then(|v| v.transaction_id);
      MembershipPurchases::insert(MembershipPurchasesActiveModel {
        code: Set(order_code.clone()),
        kind: Set(PlanKind::Subscription),
        plan_id: Set(plan.id),
        plan_code: Set(plan.code.clone()),
        owner_kind: Set(row.owner_kind.clone()),
        owner_id: Set(row.owner_id),
        status: Set(PurchaseStatus::Completed),
        amount: Set(plan.price),
        subtotal: Set(plan.price),
        currency: Set(plan.currency.clone()),
        provider: Set(provider.name().to_string()),
        provider_payment_id: Set(txn_id),
        paid_at: Set(Some(now)),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      SubscriptionEvents::insert(SubscriptionEventsActiveModel {
        subscription_id: Set(row.id),
        event_kind: Set(SubscriptionEventKind::Renewed),
        old_status: Set(Some("past-due".to_string())),
        new_status: Set(Some("active".to_string())),
        reason: Set(Some("dunning-recovered".to_string())),
        reference_code: Set(Some(order_code)),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      txn.commit().await?;
      recovered += 1;
    } else {
      let attempts = row.dunning_attempts + 1;
      Subscriptions::update(SubscriptionsActiveModel {
        id: Unchanged(row.id),
        dunning_attempts: Set(attempts),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      SubscriptionEvents::insert(SubscriptionEventsActiveModel {
        subscription_id: Set(row.id),
        event_kind: Set(SubscriptionEventKind::StatusChanged),
        old_status: Set(Some("past-due".to_string())),
        new_status: Set(Some("past-due".to_string())),
        reason: Set(Some(format!("dunning-retry-failed:{attempts}"))),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      txn.commit().await?;
      failed += 1;
    }
  }

  Ok((recovered, failed))
}

/// Transition subscriptions whose period has ended: active ones become `canceled`
/// (if flagged `cancel_at_period_end`) or `expired`; `past_due` ones whose grace
/// window has closed (or never opened) become `expired`. Each row is locked and
/// re-checked inside its own transaction, with a matching audit event.
pub async fn expire_due_subscriptions(db: &DatabaseConnection) -> Result<usize, SumidaError> {
  let now: DateTimeWithTimeZone = Utc::now().into();
  let mut total = 0usize;

  loop {
    // Active subscriptions past their period end, OR past_due ones whose grace
    // window has closed (or never opened).
    let due = Subscriptions::find()
      .filter(
        Condition::any()
          .add(
            Condition::all()
              .add(SubColumn::Status.eq(SubscriptionStatus::Active))
              .add(SubColumn::CurrentPeriodEnd.lte(now)),
          )
          .add(
            Condition::all().add(SubColumn::Status.eq(SubscriptionStatus::PastDue)).add(
              Condition::any()
                .add(SubColumn::GraceEndsAt.is_null())
                .add(SubColumn::GraceEndsAt.lte(now)),
            ),
          ),
      )
      .order_by_asc(SubColumn::Id)
      .limit(BATCH)
      .all(db)
      .await?;
    if due.is_empty() {
      break;
    }
    let batch_len = due.len();

    for sub in due {
      let txn = db.begin().await?;
      // Re-read under a row lock: a renew/dunning recovery may have intervened.
      let Some(row) = Subscriptions::find_by_id(sub.id).lock_exclusive().one(&txn).await? else {
        txn.rollback().await?;
        continue;
      };

      let active_due = row.status == SubscriptionStatus::Active && row.current_period_end <= now;
      let pastdue_done = row.status == SubscriptionStatus::PastDue
        && row.grace_ends_at.map(|g| g <= now).unwrap_or(true);
      if !(active_due || pastdue_done) {
        txn.rollback().await?;
        continue;
      }

      let (new_status, new_status_str, kind, reason, old_status_str) = if active_due {
        if row.cancel_at_period_end {
          (
            SubscriptionStatus::Canceled,
            "canceled",
            SubscriptionEventKind::Canceled,
            Some("cancel-at-period-end-reached"),
            "active",
          )
        } else {
          (SubscriptionStatus::Expired, "expired", SubscriptionEventKind::Expired, None, "active")
        }
      } else {
        // past_due with grace ended → final expiry (dunning exhausted / no grace).
        (SubscriptionStatus::Expired, "expired", SubscriptionEventKind::Expired, Some("grace-ended"), "past-due")
      };

      let mut update = SubscriptionsActiveModel {
        id: Unchanged(row.id),
        status: Set(new_status.clone()),
        ..Default::default()
      };
      if new_status == SubscriptionStatus::Canceled {
        update.canceled_at = Set(Some(now));
      }
      Subscriptions::update(update).exec(&txn).await?;

      SubscriptionEvents::insert(SubscriptionEventsActiveModel {
        subscription_id: Set(row.id),
        event_kind: Set(kind),
        old_status: Set(Some(old_status_str.to_string())),
        new_status: Set(Some(new_status_str.to_string())),
        reason: Set(reason.map(|s| s.to_string())),
        ..Default::default()
      })
      .exec(&txn)
      .await?;

      txn.commit().await?;
      total += 1;
    }

    if (batch_len as u64) < BATCH {
      break;
    }
  }

  Ok(total)
}

/// Release credit reservations whose `expires_at` has passed (stale holds, e.g.
/// from a crashed async job). A bulk status flip frees their spendable amount;
/// lots are untouched. Returns the number released.
pub async fn release_expired_reservations(db: &DatabaseConnection) -> Result<usize, SumidaError> {
  let now: DateTimeWithTimeZone = Utc::now().into();
  let res = CreditReservations::update_many()
    .col_expr(CreditReservationsColumn::Status, Expr::value("released"))
    .filter(CreditReservationsColumn::Status.eq(ReservationStatus::Held))
    .filter(CreditReservationsColumn::ExpiresAt.is_not_null())
    .filter(CreditReservationsColumn::ExpiresAt.lte(now))
    .exec(db)
    .await?;
  Ok(res.rows_affected as usize)
}

/// Expire lapsed credit lots across all wallets that have any: zero the lots,
/// write `expire` ledger rows, and correct the cached balance. Wallets are only
/// otherwise swept when touched, so inactive wallets need this.
pub async fn sweep_expired_lots<S: SumidaState>(db: &DatabaseConnection) -> Result<usize, SumidaError> {
  let now: DateTimeWithTimeZone = Utc::now().into();

  let wallet_ids: Vec<i32> = CreditGrants::find()
    .select_only()
    .column(CreditGrantsColumn::WalletId)
    .distinct()
    .filter(CreditGrantsColumn::AmountRemaining.gt(0_i64))
    .filter(CreditGrantsColumn::ExpiryAt.is_not_null())
    .filter(CreditGrantsColumn::ExpiryAt.lte(now))
    .into_tuple()
    .all(db)
    .await?;

  let mut swept = 0usize;
  for wallet_id in wallet_ids {
    let txn = db.begin().await?;
    CreditService::<S>::sweep_wallet_in_txn(wallet_id, &txn).await?;
    txn.commit().await?;
    swept += 1;
  }
  Ok(swept)
}

/// Notify personal owners whose subscription ends within
/// `expiring_soon_notify_days`. The `expiring_notified_at` marker (cleared on
/// renew) ensures at most one notice per period. Org owners are skipped (no email
/// channel in this model). Subscriptions already flagged for cancellation are not
/// nagged.
pub async fn notify_expiring_subscriptions<S: SumidaState>(
  db: &DatabaseConnection,
) -> Result<usize, SumidaError> {
  let days = settings::get().membership.expiring_soon_notify_days.max(0);
  let now = Utc::now();
  let now_dt: DateTimeWithTimeZone = now.into();
  let horizon: DateTimeWithTimeZone = (now + Duration::days(days as i64)).into();

  let due = Subscriptions::find()
    .filter(SubColumn::Status.eq(SubscriptionStatus::Active))
    .filter(SubColumn::OwnerKind.eq(OwnerKind::Personal))
    .filter(SubColumn::CancelAtPeriodEnd.eq(false))
    .filter(SubColumn::CurrentPeriodEnd.gt(now_dt))
    .filter(SubColumn::CurrentPeriodEnd.lte(horizon))
    .filter(SubColumn::ExpiringNotifiedAt.is_null())
    .all(db)
    .await?;

  let mut sent = 0usize;
  for sub in due {
    // Mark first so a slow/duplicated send never double-notifies.
    Subscriptions::update(SubscriptionsActiveModel {
      id: Unchanged(sub.id),
      expiring_notified_at: Set(Some(now_dt)),
      ..Default::default()
    })
    .exec(db)
    .await?;

    let email = Users::find_by_id(sub.owner_id).one(db).await?.and_then(|u| u.email);
    notify::<S, _, _>(
      "subscription-expiring-soon",
      email.as_deref(),
      json!({ "plan_code": sub.plan_code, "current_period_end": sub.current_period_end.to_rfc3339() }),
      db,
    )
    .await;
    sent += 1;
  }
  Ok(sent)
}

/// Notify personal owners whose wallet balance has fallen to or below
/// `credit_low_balance_threshold`. Disabled when the threshold is unset. The
/// `low_balance_notified_at` marker (cleared on top-up above the threshold)
/// ensures at most one alert per dip.
pub async fn notify_low_balance_wallets<S: SumidaState>(
  db: &DatabaseConnection,
) -> Result<usize, SumidaError> {
  let Some(threshold) = settings::get().membership.credit_low_balance_threshold else {
    return Ok(0);
  };
  let now_dt: DateTimeWithTimeZone = Utc::now().into();

  let wallets = CreditWallets::find()
    .filter(CreditWalletsColumn::OwnerKind.eq(OwnerKind::Personal))
    .filter(CreditWalletsColumn::Balance.lte(threshold))
    .filter(CreditWalletsColumn::LowBalanceNotifiedAt.is_null())
    .all(db)
    .await?;

  let mut sent = 0usize;
  for wallet in wallets {
    // Mark first so a slow/duplicated send never double-alerts.
    CreditWallets::update(CreditWalletsActiveModel {
      id: Unchanged(wallet.id),
      low_balance_notified_at: Set(Some(now_dt)),
      ..Default::default()
    })
    .exec(db)
    .await?;

    let email = Users::find_by_id(wallet.owner_id).one(db).await?.and_then(|u| u.email);
    notify::<S, _, _>(
      "credit-low-balance-alert",
      email.as_deref(),
      json!({ "balance": wallet.balance }),
      db,
    )
    .await;
    sent += 1;
  }
  Ok(sent)
}

/// Run one maintenance pass. Jobs are independent: each failure is logged and the
/// rest still run. Ordered so auto-renew runs before expiry (a successful renew
/// pre-empts expiry) and balances are corrected (sweep) before the low-balance
/// check reads them.
pub async fn run_once<S: SumidaState>(db: &DatabaseConnection) -> MaintenanceReport {
  let mut report = MaintenanceReport::default();

  match renew_due_subscriptions(db).await {
    Ok((renewed, failed)) => {
      report.subscriptions_renewed = renewed;
      report.subscriptions_renew_failed = failed;
    }
    Err(e) => tracing::warn!("membership maintenance: renew subscriptions failed: {e}"),
  }
  match retry_dunning_subscriptions(db).await {
    Ok((recovered, failed)) => {
      report.dunning_recovered = recovered;
      report.dunning_failed = failed;
    }
    Err(e) => tracing::warn!("membership maintenance: dunning retries failed: {e}"),
  }
  match expire_due_subscriptions(db).await {
    Ok(n) => report.subscriptions_expired = n,
    Err(e) => tracing::warn!("membership maintenance: expire subscriptions failed: {e}"),
  }
  match release_expired_reservations(db).await {
    Ok(n) => report.reservations_released = n,
    Err(e) => tracing::warn!("membership maintenance: release reservations failed: {e}"),
  }
  match sweep_expired_lots::<S>(db).await {
    Ok(n) => report.wallets_swept = n,
    Err(e) => tracing::warn!("membership maintenance: sweep expired lots failed: {e}"),
  }
  match notify_expiring_subscriptions::<S>(db).await {
    Ok(n) => report.expiring_notices = n,
    Err(e) => tracing::warn!("membership maintenance: expiring notices failed: {e}"),
  }
  match notify_low_balance_wallets::<S>(db).await {
    Ok(n) => report.low_balance_notices = n,
    Err(e) => tracing::warn!("membership maintenance: low-balance notices failed: {e}"),
  }

  report
}

/// Spawn the maintenance loop on a fixed interval (the first tick fires
/// immediately). Returns the task handle; dropping it leaves the task running.
/// Downstream apps call this once after building their state, e.g.
/// `membership::maintenance::spawn(state.clone(), Duration::from_secs(3600))`.
pub fn spawn<S: SumidaState>(state: S, interval: StdDuration) -> tokio::task::JoinHandle<()> {
  tokio::spawn(async move {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
      ticker.tick().await;
      let report = run_once::<S>(state.db()).await;
      tracing::debug!(
        "membership maintenance: {} renewed, {} renew-failed, {} dunning-recovered, {} dunning-failed, {} expired, {} reservations released, {} wallets swept, {} expiring notices, {} low-balance notices",
        report.subscriptions_renewed,
        report.subscriptions_renew_failed,
        report.dunning_recovered,
        report.dunning_failed,
        report.subscriptions_expired,
        report.reservations_released,
        report.wallets_swept,
        report.expiring_notices,
        report.low_balance_notices,
      );
    }
  })
}
