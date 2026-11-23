//! Amount quotas: what a live subscription lets an owner spend on an action per period, metered
//! by amount, held, extended, captured and released the way a credit wallet is.

pub mod dto;

use chrono::Utc;
use entity::membership::{OwnerKind, QuotaEntryKind, ReservationStatus};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  EntityTrait,
  FromQueryResult,
  QueryFilter,
  QuerySelect,
  Statement,
  TransactionTrait,
};

use self::dto::*;
use super::credit::CreditService;
use super::credit::dto::{ExtendReservation, ReleaseReservation};
use super::{authorize_overrun, hold_expiry, owner_kind_str, resolve_owner};
use crate::SumidaState;
use crate::authz::subject::SubjectSet;

type Reservation = entity::membership::quota_reservations::Model;

/// The live subscription covering an action, and what its plan allows per period.
struct Coverage {
  subscription_id: i32,
  period_start:    DateTimeUtc,
  /// Uses per period; `None` = unlimited.
  count_quota:     Option<i64>,
  /// Amount per period; `None` = unlimited.
  amount_quota:    Option<i64>,
}

/// One period's usage row, under its row lock.
struct Usage {
  id:          i64,
  used:        i64,
  amount_used: i64,
}

/// What a period has left: `None` when it has no amount limit.
fn room(limit: Option<i64>, usage: &Usage, held: i64) -> Option<i64> {
  limit.map(|limit| limit - usage.amount_used - held)
}

/// One quota ledger entry to append.
struct Entry<'a> {
  kind:            QuotaEntryKind,
  amount_change:   i64,
  held_change:     i64,
  reservation_id:  Option<i32>,
  reference_code:  Option<&'a str>,
  idempotency_key: Option<&'a str>,
}

impl Entry<'_> {
  fn of(kind: QuotaEntryKind) -> Self {
    Self {
      kind,
      amount_change: 0,
      held_change: 0,
      reservation_id: None,
      reference_code: None,
      idempotency_key: None,
    }
  }
}

pub struct QuotaService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> QuotaService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Spending against a subscription's quota is authorized as spending its owner's credit is:
  /// the owner, a member of the owning organization, or a platform administrator.
  async fn authorize_use<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<(), SumidaError> {
    CreditService::new(self.state.clone(), self.session.clone())
      .authorize_use(owner_kind, owner_id, db)
      .await
  }

  // ---- helpers ----

  /// The live subscription that covers `action` for the owner — active within its period, or
  /// past due inside an open grace window — the one ending last when several do.
  async fn coverage<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    db: &C,
  ) -> Result<Option<Coverage>, SumidaError> {
    #[derive(FromQueryResult)]
    struct Row {
      id:                   i32,
      current_period_start: DateTimeUtc,
      count_quota:          Option<serde_json::Value>,
      amount_quota:         Option<serde_json::Value>,
    }
    let backend = db.get_database_backend();
    let row = Row::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT id, current_period_start,
                (allowed_actions -> $3) AS count_quota,
                (amount_quotas -> $3) AS amount_quota
         FROM subscriptions
         WHERE owner_kind = $1 AND owner_id = $2
           AND jsonb_exists(allowed_actions, $3)
           AND (
             (status = 'active' AND current_period_end > now())
             OR (status = 'past-due' AND grace_ends_at IS NOT NULL AND grace_ends_at > now())
           )
         ORDER BY current_period_end DESC
         LIMIT 1"#,
      [owner_kind_str(owner_kind).into(), owner_id.into(), action.into()],
    ))
    .one(db)
    .await?;
    Ok(row.map(|r| Coverage {
      subscription_id: r.id,
      period_start:    r.current_period_start,
      count_quota:     r.count_quota.and_then(|v| v.as_i64()),
      amount_quota:    r.amount_quota.and_then(|v| v.as_i64()),
    }))
  }

  /// The amount limit a subscription's snapshot carries for an action.
  async fn amount_limit<C: ConnectionTrait>(
    subscription_id: i32,
    action: &str,
    db: &C,
  ) -> Result<Option<i64>, SumidaError> {
    #[derive(FromQueryResult)]
    struct Row {
      amount_quota: Option<serde_json::Value>,
    }
    let backend = db.get_database_backend();
    let row = Row::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT (amount_quotas -> $2) AS amount_quota FROM subscriptions WHERE id = $1"#,
      [subscription_id.into(), action.into()],
    ))
    .one(db)
    .await?;
    Ok(row.and_then(|r| r.amount_quota).and_then(|v| v.as_i64()))
  }

  /// The usage row of (subscription, action, period), created when absent and locked, so every
  /// step on the period runs one at a time.
  async fn lock_usage<C: ConnectionTrait>(
    subscription_id: i32,
    action: &str,
    period_start: DateTimeUtc,
    txn: &C,
  ) -> Result<Usage, SumidaError> {
    let backend = txn.get_database_backend();
    txn
      .execute(Statement::from_sql_and_values(
        backend,
        r#"INSERT INTO subscription_usage (subscription_id, action, period_start, used, amount_used)
           VALUES ($1, $2, $3, 0, 0)
           ON CONFLICT (subscription_id, action, period_start) DO NOTHING"#,
        [subscription_id.into(), action.into(), period_start.into()],
      ))
      .await?;
    let row = SubscriptionUsage::find()
      .filter(SubscriptionUsageColumn::SubscriptionId.eq(subscription_id))
      .filter(SubscriptionUsageColumn::Action.eq(action))
      .filter(SubscriptionUsageColumn::PeriodStart.eq(period_start))
      .lock_exclusive()
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::InternalServerError("usage row vanished".into()))?;
    Ok(Usage {
      id:          row.id,
      used:        row.used,
      amount_used: row.amount_used,
    })
  }

  /// What live holds take out of a period's room: held and not past their expiry.
  async fn held<C: ConnectionTrait>(
    subscription_id: i32,
    action: &str,
    period_start: DateTimeUtc,
    txn: &C,
  ) -> Result<i64, SumidaError> {
    #[derive(FromQueryResult)]
    struct SumRow {
      total: i64,
    }
    let backend = txn.get_database_backend();
    let row = SumRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT COALESCE(SUM(amount), 0)::bigint AS total
         FROM quota_reservations
         WHERE subscription_id = $1 AND action = $2 AND period_start = $3
           AND status = 'held' AND expires_at > now()"#,
      [subscription_id.into(), action.into(), period_start.into()],
    ))
    .one(txn)
    .await?;
    Ok(row.map(|r| r.total).unwrap_or(0))
  }

  async fn add_usage<C: ConnectionTrait>(
    usage: &Usage,
    uses: i64,
    amount: i64,
    txn: &C,
  ) -> Result<(), SumidaError> {
    SubscriptionUsage::update(SubscriptionUsageActiveModel {
      id: Unchanged(usage.id),
      used: Set(usage.used + uses),
      amount_used: Set(usage.amount_used + amount),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Ok(())
  }

  async fn record<C: ConnectionTrait>(
    subscription_id: i32,
    action: &str,
    period_start: DateTimeUtc,
    entry: Entry<'_>,
    txn: &C,
  ) -> Result<(), SumidaError> {
    QuotaEntries::insert(QuotaEntriesActiveModel {
      subscription_id: Set(subscription_id),
      action: Set(action.to_string()),
      period_start: Set(period_start),
      kind: Set(entry.kind),
      amount_change: Set(entry.amount_change),
      held_change: Set(entry.held_change),
      reservation_id: Set(entry.reservation_id),
      reference_code: Set(entry.reference_code.map(str::to_string)),
      idempotency_key: Set(entry.idempotency_key.map(str::to_string)),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Ok(())
  }

  async fn entry_seen<C: ConnectionTrait>(
    subscription_id: i32,
    key: Option<&str>,
    txn: &C,
  ) -> Result<bool, SumidaError> {
    let Some(key) = key else {
      return Ok(false);
    };
    Ok(
      QuotaEntries::find()
        .filter(QuotaEntriesColumn::SubscriptionId.eq(subscription_id))
        .filter(QuotaEntriesColumn::IdempotencyKey.eq(key))
        .one(txn)
        .await?
        .is_some(),
    )
  }

  /// The hold, its period's usage locked, and the hold read again under its own row lock.
  async fn lock_reservation<C: ConnectionTrait>(
    reservation_id: i32,
    txn: &C,
  ) -> Result<(Usage, Reservation), SumidaError> {
    let missing = || SumidaError::NoData("QuotaReservation".into(), reservation_id.to_string());
    let res = QuotaReservations::find_by_id(reservation_id).one(txn).await?.ok_or_else(missing)?;
    let usage = Self::lock_usage(res.subscription_id, &res.action, res.period_start, txn).await?;
    let res = QuotaReservations::find_by_id(reservation_id)
      .lock_exclusive()
      .one(txn)
      .await?
      .ok_or_else(missing)?;
    Ok((usage, res))
  }

  fn ensure_live(res: &Reservation) -> Result<(), SumidaError> {
    if res.status != ReservationStatus::Held {
      return Err(SumidaError::BadRequest("reservation is not held".into()));
    }
    if res.expires_at <= Utc::now() {
      return Err(SumidaError::ReservationExpired(res.id));
    }
    Ok(())
  }

  /// The hold as it stands, read with its period's figures.
  async fn result<C: ConnectionTrait>(
    res: &Reservation,
    txn: &C,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let limit = Self::amount_limit(res.subscription_id, &res.action, txn).await?;
    let usage = SubscriptionUsage::find()
      .filter(SubscriptionUsageColumn::SubscriptionId.eq(res.subscription_id))
      .filter(SubscriptionUsageColumn::Action.eq(res.action.clone()))
      .filter(SubscriptionUsageColumn::PeriodStart.eq(res.period_start))
      .one(txn)
      .await?;
    let usage = Usage {
      id:          usage.as_ref().map(|u| u.id).unwrap_or(0),
      used:        usage.as_ref().map(|u| u.used).unwrap_or(0),
      amount_used: usage.as_ref().map(|u| u.amount_used).unwrap_or(0),
    };
    let held = Self::held(res.subscription_id, &res.action, res.period_start, txn).await?;
    Ok(QuotaReservationResult {
      reservation_id: res.id,
      subscription_id: res.subscription_id,
      action: res.action.clone(),
      amount: res.amount,
      status: res.status.clone(),
      expires_at: res.expires_at,
      period_start: res.period_start,
      limit,
      amount_used: usage.amount_used,
      held,
      remaining: room(limit, &usage, held).map(|r| r.max(0)),
    })
  }

  /// Who owns the subscription a hold is against, for authorizing a step on it.
  async fn reservation_owner<C: ConnectionTrait>(
    reservation_id: i32,
    db: &C,
  ) -> Result<(OwnerKind, i32), SumidaError> {
    let Some(res) = QuotaReservations::find_by_id(reservation_id).one(db).await? else {
      return Err(SumidaError::NoData("QuotaReservation".into(), reservation_id.to_string()));
    };
    let Some(sub) = Subscriptions::find_by_id(res.subscription_id).one(db).await? else {
      return Err(SumidaError::NoData("Subscription".into(), res.subscription_id.to_string()));
    };
    Ok((sub.owner_kind, sub.owner_id))
  }

  // ---- core steps, authorized by the caller ----

  /// The entitlement gate's subscription path. When a live subscription covers `action` and
  /// both its count quota and, for an `amount`, its amount room allow, records one use and the
  /// amount and answers `true`; otherwise `false`, changing nothing, so the gate can fall
  /// through to credits. A key seen before on the subscription answers `true` again without
  /// recording.
  pub(crate) async fn consume_in_txn<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    amount: Option<i64>,
    idempotency_key: Option<&str>,
    txn: &C,
  ) -> Result<bool, SumidaError> {
    if amount.is_some_and(|a| a < 0) {
      return Err(SumidaError::BadRequest("amount must be non-negative".into()));
    }
    let Some(cov) = Self::coverage(owner_kind, owner_id, action, txn).await? else {
      return Ok(false);
    };
    if cov.count_quota.is_some_and(|q| q <= 0) {
      return Ok(false);
    }
    // Nothing to meter and nothing to remember: covered, as an unlimited action always was.
    if cov.count_quota.is_none() && amount.is_none() && idempotency_key.is_none() {
      return Ok(true);
    }

    let usage = Self::lock_usage(cov.subscription_id, action, cov.period_start, txn).await?;
    if Self::entry_seen(cov.subscription_id, idempotency_key, txn).await? {
      return Ok(true);
    }
    if cov.count_quota.is_some_and(|q| usage.used >= q) {
      return Ok(false);
    }
    if let Some(amount) = amount {
      let held = Self::held(cov.subscription_id, action, cov.period_start, txn).await?;
      if room(cov.amount_quota, &usage, held).is_some_and(|room| amount > room) {
        return Ok(false);
      }
    }

    let uses = i64::from(cov.count_quota.is_some());
    let amount = amount.unwrap_or(0);
    Self::add_usage(&usage, uses, amount, txn).await?;
    Self::record(
      cov.subscription_id,
      action,
      cov.period_start,
      Entry {
        amount_change: amount,
        idempotency_key,
        ..Entry::of(QuotaEntryKind::Consume)
      },
      txn,
    )
    .await?;
    Ok(true)
  }

  /// Hold `amount` of the period's room. 403 without a live subscription covering the action,
  /// 402 when the room is short. Idempotent per (subscription, key).
  #[allow(clippy::too_many_arguments)]
  pub async fn reserve_in_txn<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    amount: i64,
    reference_code: Option<&str>,
    idempotency_key: Option<&str>,
    expires_in_secs: Option<i64>,
    txn: &C,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let Some(cov) = Self::coverage(owner_kind, owner_id, action, txn).await? else {
      return Err(SumidaError::NoActiveSubscription);
    };
    let usage = Self::lock_usage(cov.subscription_id, action, cov.period_start, txn).await?;

    if let Some(key) = idempotency_key
      && let Some(prev) = QuotaReservations::find()
        .filter(QuotaReservationsColumn::SubscriptionId.eq(cov.subscription_id))
        .filter(QuotaReservationsColumn::IdempotencyKey.eq(key))
        .one(txn)
        .await?
    {
      return Self::result(&prev, txn).await;
    }

    if amount <= 0 {
      return Err(SumidaError::BadRequest("a hold takes a positive amount".into()));
    }
    let held = Self::held(cov.subscription_id, action, cov.period_start, txn).await?;
    if let Some(room) = room(cov.amount_quota, &usage, held)
      && amount > room
    {
      return Err(SumidaError::QuotaExceeded {
        required:  amount,
        available: room.max(0),
      });
    }

    let res = QuotaReservations::insert(QuotaReservationsActiveModel {
      subscription_id: Set(cov.subscription_id),
      action: Set(action.to_string()),
      period_start: Set(cov.period_start),
      amount: Set(amount),
      status: Set(ReservationStatus::Held),
      reference_code: Set(reference_code.map(str::to_string)),
      idempotency_key: Set(idempotency_key.map(str::to_string)),
      expires_at: Set(hold_expiry(expires_in_secs)?),
      ..Default::default()
    })
    .exec_with_returning(txn)
    .await?;
    Self::record(
      cov.subscription_id,
      action,
      cov.period_start,
      Entry {
        held_change: amount,
        reservation_id: Some(res.id),
        reference_code,
        ..Entry::of(QuotaEntryKind::Reserve)
      },
      txn,
    )
    .await?;

    Self::result(&res, txn).await
  }

  /// Grow a live hold by `amount` and restart its expiry. 402 when the room is short.
  /// Idempotent per (subscription, key).
  pub async fn extend_in_txn<C: ConnectionTrait>(
    reservation_id: i32,
    amount: i64,
    idempotency_key: Option<&str>,
    expires_in_secs: Option<i64>,
    txn: &C,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let (usage, res) = Self::lock_reservation(reservation_id, txn).await?;
    if Self::entry_seen(res.subscription_id, idempotency_key, txn).await? {
      return Self::result(&res, txn).await;
    }
    Self::ensure_live(&res)?;
    if amount <= 0 {
      return Err(SumidaError::BadRequest("an extension takes a positive amount".into()));
    }
    let limit = Self::amount_limit(res.subscription_id, &res.action, txn).await?;
    let held = Self::held(res.subscription_id, &res.action, res.period_start, txn).await?;
    if let Some(room) = room(limit, &usage, held)
      && amount > room
    {
      return Err(SumidaError::QuotaExceeded {
        required:  amount,
        available: room.max(0),
      });
    }

    let res = QuotaReservations::update(QuotaReservationsActiveModel {
      id: Unchanged(res.id),
      amount: Set(res.amount + amount),
      expires_at: Set(hold_expiry(expires_in_secs)?),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Self::record(
      res.subscription_id,
      &res.action,
      res.period_start,
      Entry {
        held_change: amount,
        reservation_id: Some(res.id),
        reference_code: res.reference_code.as_deref(),
        idempotency_key,
        ..Entry::of(QuotaEntryKind::Extend)
      },
      txn,
    )
    .await?;

    Self::result(&res, txn).await
  }

  /// Record a live hold as usage at the amount actually used, in the period it was taken in.
  /// Past the period's room the usage goes up to `overage_limit` further and is refused with 402
  /// beyond, changing nothing. Idempotent per (subscription, key).
  pub async fn capture_in_txn<C: ConnectionTrait>(
    reservation_id: i32,
    amount: Option<i64>,
    overage_limit: i64,
    idempotency_key: Option<&str>,
    txn: &C,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let (usage, res) = Self::lock_reservation(reservation_id, txn).await?;
    if Self::entry_seen(res.subscription_id, idempotency_key, txn).await? {
      return Self::result(&res, txn).await;
    }
    Self::ensure_live(&res)?;
    let actual = amount.unwrap_or(res.amount);
    if actual < 0 {
      return Err(SumidaError::BadRequest("amount must be non-negative".into()));
    }

    let held_amount = res.amount;
    let res = QuotaReservations::update(QuotaReservationsActiveModel {
      id: Unchanged(res.id),
      amount: Set(actual),
      status: Set(ReservationStatus::Captured),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    let limit = Self::amount_limit(res.subscription_id, &res.action, txn).await?;
    let held = Self::held(res.subscription_id, &res.action, res.period_start, txn).await?;
    if let Some(room) = room(limit, &usage, held) {
      let overage = actual - room.max(0);
      if overage > overage_limit.max(0) {
        return Err(SumidaError::QuotaExceeded {
          required:  actual,
          available: room.max(0),
        });
      }
    }

    Self::add_usage(&usage, 0, actual, txn).await?;
    Self::record(
      res.subscription_id,
      &res.action,
      res.period_start,
      Entry {
        amount_change: actual,
        held_change: -held_amount,
        reservation_id: Some(res.id),
        reference_code: res.reference_code.as_deref(),
        idempotency_key,
        ..Entry::of(QuotaEntryKind::Capture)
      },
      txn,
    )
    .await?;

    Self::result(&res, txn).await
  }

  /// Drop a held quota hold, expired or not. Idempotent per (subscription, key).
  pub async fn release_in_txn<C: ConnectionTrait>(
    reservation_id: i32,
    idempotency_key: Option<&str>,
    txn: &C,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let (_usage, res) = Self::lock_reservation(reservation_id, txn).await?;
    if Self::entry_seen(res.subscription_id, idempotency_key, txn).await? {
      return Self::result(&res, txn).await;
    }
    if res.status != ReservationStatus::Held {
      return Err(SumidaError::BadRequest("reservation is not held".into()));
    }

    let held_amount = res.amount;
    let res = QuotaReservations::update(QuotaReservationsActiveModel {
      id: Unchanged(res.id),
      status: Set(ReservationStatus::Released),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Self::record(
      res.subscription_id,
      &res.action,
      res.period_start,
      Entry {
        held_change: -held_amount,
        reservation_id: Some(res.id),
        reference_code: res.reference_code.as_deref(),
        idempotency_key,
        ..Entry::of(QuotaEntryKind::Release)
      },
      txn,
    )
    .await?;

    Self::result(&res, txn).await
  }

  // ---- public API ----

  pub async fn reserve(&self, data: ReserveQuota) -> Result<QuotaReservationResult, SumidaError> {
    let (owner_kind, owner_id) =
      resolve_owner(&self.session, &data.owner_kind, data.owner_id, self.state.db()).await?;
    let txn = self.state.db().begin().await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let result = Self::reserve_in_txn(
      &owner_kind,
      owner_id,
      &data.action,
      data.amount,
      data.reference_code.as_deref(),
      data.idempotency_key.as_deref(),
      data.expires_in_secs,
      &txn,
    )
    .await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn extend(
    &self,
    reservation_id: i32,
    data: ExtendReservation,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let (owner_kind, owner_id) = Self::reservation_owner(reservation_id, &txn).await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let result = Self::extend_in_txn(
      reservation_id,
      data.amount,
      data.idempotency_key.as_deref(),
      data.expires_in_secs,
      &txn,
    )
    .await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn capture(
    &self,
    reservation_id: i32,
    data: CaptureQuota,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let (owner_kind, owner_id) = Self::reservation_owner(reservation_id, &txn).await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let overage_limit = data.overage_limit.unwrap_or(0);
    if overage_limit > 0 {
      let subject = SubjectSet::of(&self.session);
      authorize_overrun(&subject, &owner_kind, owner_id, "edit_subscription", &txn).await?;
    }
    let result = Self::capture_in_txn(
      reservation_id,
      data.amount,
      overage_limit,
      data.idempotency_key.as_deref(),
      &txn,
    )
    .await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn release(
    &self,
    reservation_id: i32,
    data: ReleaseReservation,
  ) -> Result<QuotaReservationResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let (owner_kind, owner_id) = Self::reservation_owner(reservation_id, &txn).await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let result = Self::release_in_txn(reservation_id, data.idempotency_key.as_deref(), &txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}
