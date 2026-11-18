pub mod dto;

use chrono::Utc;
use entity::membership::{CreditTxKind, OwnerKind, ReservationStatus};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
  ColumnTrait,
  Condition,
  ConnectionTrait,
  EntityTrait,
  FromQueryResult,
  PaginatorTrait,
  QueryFilter,
  QueryOrder,
  QuerySelect,
  QueryTrait,
  Statement,
  TransactionTrait,
};

use self::dto::*;
use super::{authorize_overrun, hold_expiry, notify_all, owner_kind_str, recipient_emails, resolve_owner};
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::pagination::Pagination;

/// Outcome of a FIFO debit (consume/refund).
pub struct DebitOutcome {
  pub wallet_id: i32,
  pub debited:   i64,
  pub balance:   i64,
  pub debt:      i64,
}

/// Outcome of a consume (for the entitlement gate).
pub struct ConsumeOutcome {
  pub wallet_id: i32,
  pub consumed:  i64,
  pub balance:   i64,
}

/// What a step spends: the caller's `amount`, or else the fixed cost of `action` under a credit
/// plan (the one named by `plan_code`, or the cheapest that prices it).
#[derive(Clone, Copy, Debug, Default)]
pub struct Cost<'a> {
  pub action:    Option<&'a str>,
  pub amount:    Option<i64>,
  pub plan_code: Option<&'a str>,
}

impl Cost<'_> {
  async fn resolve<C: ConnectionTrait>(&self, db: &C) -> Result<i64, SumidaError> {
    match (self.amount, self.action) {
      (Some(amount), _) if amount < 0 => Err(SumidaError::BadRequest("amount must be non-negative".into())),
      (Some(amount), _) => Ok(amount),
      (None, Some(action)) => resolve_cost(action, self.plan_code, db)
        .await?
        .ok_or_else(|| SumidaError::MembershipActionNotAllowed(action.to_string())),
      (None, None) => Err(SumidaError::BadRequest("either amount or action is required".into())),
    }
  }
}

/// A hold to place on a wallet.
#[derive(Clone, Copy, Debug, Default)]
pub struct HoldRequest<'a> {
  pub cost:            Cost<'a>,
  pub reference_code:  Option<&'a str>,
  pub idempotency_key: Option<&'a str>,
  pub expires_in_secs: Option<i64>,
}

/// A capture of a wallet hold.
#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureRequest<'a> {
  /// What was actually used; `None` is the hold's amount.
  pub amount:          Option<i64>,
  /// How much of a shortfall may become debt; 0 refuses any.
  pub debt_limit:      i64,
  pub idempotency_key: Option<&'a str>,
}

/// A wallet under its row lock, its lapsed lots swept and its free balance gone to its debt.
struct LockedWallet {
  id:         i32,
  owner_kind: OwnerKind,
  owner_id:   i32,
  balance:    i64,
  debt:       i64,
}

/// One ledger entry to append. `amount` changes the balance and `debt_change` the debt.
struct Entry<'a> {
  kind:            CreditTxKind,
  amount:          i64,
  debt_change:     i64,
  grant_id:        Option<i32>,
  subscription_id: Option<i32>,
  action:          Option<&'a str>,
  reference_code:  Option<&'a str>,
  idempotency_key: Option<&'a str>,
}

impl<'a> Entry<'a> {
  fn of(kind: CreditTxKind) -> Self {
    Self {
      kind,
      amount: 0,
      debt_change: 0,
      grant_id: None,
      subscription_id: None,
      action: None,
      reference_code: None,
      idempotency_key: None,
    }
  }
}

/// Resolve the credit cost of an action from a (optionally specified) credit plan.
/// When several credit plans define the action, the lowest cost is used.
async fn resolve_cost<C: ConnectionTrait>(
  action: &str,
  plan_code: Option<&str>,
  db: &C,
) -> Result<Option<i64>, SumidaError> {
  #[derive(FromQueryResult)]
  struct CostRow {
    credit_cost: i64,
  }
  let backend = db.get_database_backend();
  let plan_code = plan_code.map(|s| s.to_string());
  let row = CostRow::find_by_statement(Statement::from_sql_and_values(
    backend,
    r#"SELECT pa.credit_cost AS credit_cost
       FROM plan_actions pa JOIN membership_plans p ON pa.plan_id = p.id
       WHERE p.kind = 'credit' AND p.deleted_at IS NULL
         AND pa.action = $1 AND pa.credit_cost IS NOT NULL
         AND ($2::text IS NULL OR p.code = $2)
       ORDER BY pa.credit_cost ASC LIMIT 1"#,
    [action.into(), plan_code.into()],
  ))
  .one(db)
  .await?;
  Ok(row.map(|r| r.credit_cost))
}

pub struct CreditService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> CreditService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  // ---- authorization ----

  /// Manage = grant/refund: putting credits into a wallet or taking them back out.
  ///
  /// A personal wallet's owner is not among those allowed — an owner granting themselves credits
  /// would mint money. Only `platform:root#superuser` reaches a personal wallet; an organization's
  /// wallet is also reached by `organization#manage_credit` (`credit_manager^ | manage`), whose
  /// closure carries an ancestor's grant down to its descendants. Credits a payment buys or a
  /// coupon awards are granted inside the server and never pass this check.
  pub async fn authorize_manage<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<(), SumidaError> {
    if Authz::is_superuser(&self.subject(), db).await? {
      return Ok(());
    }
    match owner_kind {
      OwnerKind::Personal => Err(SumidaError::NoAccessPermission),
      OwnerKind::Organization => {
        Authz::try_check("organization", owner_id, "manage_credit", &self.subject(), db).await
      }
    }
  }

  /// Use = consume/read balance. 최상위 관리자 / 본인 / `organization#view_credit` 보유자.
  ///
  /// "같은 조직 구성원이면 통과" 는 여기서 분기로 다루지 않는다. 규칙 안에 있다 —
  /// `view_credit = credit_viewer^ | edit_credit | member` 의 `member` 갈래가 그것이고,
  /// 그래서 목록·단건 어느 쪽에서 물어도 같은 답이 나온다.
  pub async fn authorize_use<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<(), SumidaError> {
    if Authz::is_superuser(&self.subject(), db).await? {
      return Ok(());
    }
    match owner_kind {
      OwnerKind::Personal if self.session.profile.id == owner_id => Ok(()),
      OwnerKind::Personal => Err(SumidaError::NoAccessPermission),
      OwnerKind::Organization => {
        Authz::try_check("organization", owner_id, "view_credit", &self.subject(), db).await
      }
    }
  }

  /// Use, plus — when the capture lets a shortfall become debt — the right to allow that, which
  /// on an organization's wallet is `edit_credit`.
  async fn authorize_capture<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    debt_limit: i64,
    db: &C,
  ) -> Result<(), SumidaError> {
    self.authorize_use(owner_kind, owner_id, db).await?;
    if debt_limit > 0 {
      authorize_overrun(&self.subject(), owner_kind, owner_id, "edit_credit", db).await?;
    }
    Ok(())
  }

  // ---- helpers ----

  async fn find_wallet<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<Option<entity::membership::credit_wallets::Model>, SumidaError> {
    Ok(
      CreditWallets::find()
        .filter(CreditWalletsColumn::OwnerKind.eq(owner_kind.clone()))
        .filter(CreditWalletsColumn::OwnerId.eq(owner_id))
        .one(db)
        .await?,
    )
  }

  /// Sum of credits on hold for a wallet: reservations still `held` and not past their expiry. An
  /// expired hold stops counting at once, before the scheduler gets round to releasing it.
  async fn held_reserved<C: ConnectionTrait>(wallet_id: i32, txn: &C) -> Result<i64, SumidaError> {
    #[derive(FromQueryResult)]
    struct SumRow {
      total: i64,
    }
    let backend = txn.get_database_backend();
    let row = SumRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT COALESCE(SUM(amount), 0)::bigint AS total
         FROM credit_reservations
         WHERE wallet_id = $1 AND status = 'held' AND (expires_at IS NULL OR expires_at > now())"#,
      [wallet_id.into()],
    ))
    .one(txn)
    .await?;
    Ok(row.map(|r| r.total).unwrap_or(0))
  }

  /// What the wallet can spend now: its balance less its live holds and its debt.
  async fn spendable<C: ConnectionTrait>(wallet: &LockedWallet, txn: &C) -> Result<i64, SumidaError> {
    let held = Self::held_reserved(wallet.id, txn).await?;
    Ok((wallet.balance - held - wallet.debt).max(0))
  }

  /// Idempotent wallet creation: INSERT … ON CONFLICT DO UPDATE … RETURNING id.
  async fn ensure_wallet_id<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<i32, SumidaError> {
    #[derive(FromQueryResult)]
    struct IdRow {
      id: i32,
    }
    let backend = db.get_database_backend();
    let row = IdRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"INSERT INTO credit_wallets (owner_kind, owner_id, balance)
         VALUES ($1, $2, 0)
         ON CONFLICT (owner_kind, owner_id) DO UPDATE SET updated_at = now()
         RETURNING id"#,
      [owner_kind_str(owner_kind).into(), owner_id.into()],
    ))
    .one(db)
    .await?;
    row
      .map(|r| r.id)
      .ok_or_else(|| SumidaError::InternalServerError("wallet upsert failed".into()))
  }

  /// Expire any lapsed lots: zero their remaining, write `expire` ledger rows, and
  /// decrement the cached wallet balance. Returns the post-sweep balance. The caller
  /// must already hold the wallet row lock (this runs inside the same transaction).
  async fn sweep_expired<C: ConnectionTrait>(
    wallet_id: i32,
    current_balance: i64,
    txn: &C,
  ) -> Result<i64, SumidaError> {
    let now = Utc::now();
    let expired = CreditGrants::find()
      .filter(CreditGrantsColumn::WalletId.eq(wallet_id))
      .filter(CreditGrantsColumn::AmountRemaining.gt(0_i64))
      .filter(CreditGrantsColumn::ExpiryAt.is_not_null())
      .filter(CreditGrantsColumn::ExpiryAt.lte(now))
      .lock_exclusive()
      .all(txn)
      .await?;

    if expired.is_empty() {
      return Ok(current_balance);
    }

    let mut balance = current_balance;
    for lot in expired {
      let amt = lot.amount_remaining;
      CreditGrants::update(CreditGrantsActiveModel {
        id: Unchanged(lot.id),
        amount_remaining: Set(0),
        ..Default::default()
      })
      .exec(txn)
      .await?;
      balance -= amt;
      CreditTransactions::insert(CreditTransactionsActiveModel {
        wallet_id: Set(wallet_id),
        grant_id: Set(Some(lot.id)),
        kind: Set(CreditTxKind::Expire),
        amount: Set(-amt),
        balance_after: Set(balance),
        ..Default::default()
      })
      .exec(txn)
      .await?;
    }

    CreditWallets::update(CreditWalletsActiveModel {
      id: Unchanged(wallet_id),
      balance: Set(balance),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    Ok(balance)
  }

  /// Lock a wallet row, sweep its lapsed lots, and repay its debt from whatever balance no live
  /// hold takes.
  async fn lock_wallet<C: ConnectionTrait>(wallet_id: i32, txn: &C) -> Result<LockedWallet, SumidaError> {
    let row = CreditWallets::find_by_id(wallet_id)
      .lock_exclusive()
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("CreditWallet".into(), wallet_id.to_string()))?;
    let balance = Self::sweep_expired(row.id, row.balance, txn).await?;
    let mut wallet = LockedWallet {
      id: row.id,
      owner_kind: row.owner_kind,
      owner_id: row.owner_id,
      balance,
      debt: row.debt,
    };
    if Self::settle_debt(&mut wallet, txn).await? {
      Self::store_wallet(&wallet, txn).await?;
    }
    Ok(wallet)
  }

  async fn lock_owner_wallet<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    txn: &C,
  ) -> Result<Option<LockedWallet>, SumidaError> {
    match Self::find_wallet(owner_kind, owner_id, txn).await? {
      Some(w) => Ok(Some(Self::lock_wallet(w.id, txn).await?)),
      None => Ok(None),
    }
  }

  /// Write the wallet's cached balance and debt back.
  async fn store_wallet<C: ConnectionTrait>(wallet: &LockedWallet, txn: &C) -> Result<(), SumidaError> {
    CreditWallets::update(CreditWalletsActiveModel {
      id: Unchanged(wallet.id),
      balance: Set(wallet.balance),
      debt: Set(wallet.debt),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Ok(())
  }

  /// Append a ledger entry, its running balance taken from the wallet as it now stands.
  async fn record<C: ConnectionTrait>(
    wallet: &LockedWallet,
    entry: Entry<'_>,
    txn: &C,
  ) -> Result<(), SumidaError> {
    CreditTransactions::insert(CreditTransactionsActiveModel {
      wallet_id: Set(wallet.id),
      grant_id: Set(entry.grant_id),
      subscription_id: Set(entry.subscription_id),
      kind: Set(entry.kind),
      amount: Set(entry.amount),
      debt_change: Set(entry.debt_change),
      action: Set(entry.action.map(str::to_string)),
      reference_code: Set(entry.reference_code.map(str::to_string)),
      idempotency_key: Set(entry.idempotency_key.map(str::to_string)),
      balance_after: Set(wallet.balance),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Ok(())
  }

  async fn entry_by_key<C: ConnectionTrait>(
    wallet_id: i32,
    key: Option<&str>,
    txn: &C,
  ) -> Result<Option<entity::membership::credit_transactions::Model>, SumidaError> {
    let Some(key) = key else {
      return Ok(None);
    };
    Ok(
      CreditTransactions::find()
        .filter(CreditTransactionsColumn::WalletId.eq(wallet_id))
        .filter(CreditTransactionsColumn::IdempotencyKey.eq(key))
        .one(txn)
        .await?,
    )
  }

  /// Draw `amount` from the wallet's live lots, soonest-expiring first. The caller has checked
  /// that the lots hold it.
  async fn take_from_lots<C: ConnectionTrait>(
    wallet_id: i32,
    amount: i64,
    txn: &C,
  ) -> Result<(), SumidaError> {
    if amount <= 0 {
      return Ok(());
    }
    let now = Utc::now();
    let lots = CreditGrants::find()
      .filter(CreditGrantsColumn::WalletId.eq(wallet_id))
      .filter(CreditGrantsColumn::AmountRemaining.gt(0_i64))
      .filter(
        Condition::any()
          .add(CreditGrantsColumn::ExpiryAt.is_null())
          .add(CreditGrantsColumn::ExpiryAt.gt(now)),
      )
      .order_by_asc(CreditGrantsColumn::ExpiryAt)
      .order_by_asc(CreditGrantsColumn::CreatedAt)
      .lock_exclusive()
      .all(txn)
      .await?;

    let mut remaining = amount;
    for lot in lots {
      if remaining <= 0 {
        break;
      }
      let take = remaining.min(lot.amount_remaining);
      CreditGrants::update(CreditGrantsActiveModel {
        id: Unchanged(lot.id),
        amount_remaining: Set(lot.amount_remaining - take),
        ..Default::default()
      })
      .exec(txn)
      .await?;
      remaining -= take;
    }
    if remaining > 0 {
      return Err(SumidaError::InternalServerError(format!(
        "credit wallet {wallet_id}: lots hold less than the cached balance"
      )));
    }
    Ok(())
  }

  /// Repay debt from the balance no live hold takes, recording a `repay` entry. Returns whether
  /// anything was repaid, so the caller knows to store the wallet.
  async fn settle_debt<C: ConnectionTrait>(
    wallet: &mut LockedWallet,
    txn: &C,
  ) -> Result<bool, SumidaError> {
    if wallet.debt <= 0 {
      return Ok(false);
    }
    let held = Self::held_reserved(wallet.id, txn).await?;
    let repay = wallet.debt.min((wallet.balance - held).max(0));
    if repay == 0 {
      return Ok(false);
    }
    Self::take_from_lots(wallet.id, repay, txn).await?;
    wallet.balance -= repay;
    wallet.debt -= repay;
    Self::record(
      wallet,
      Entry {
        amount: -repay,
        debt_change: -repay,
        ..Entry::of(CreditTxKind::Repay)
      },
      txn,
    )
    .await?;
    Ok(true)
  }

  /// Background-maintenance entrypoint: lock a wallet and expire its lapsed lots.
  /// Returns the post-sweep balance. Runs inside the caller's transaction.
  pub(crate) async fn sweep_wallet_in_txn<C: ConnectionTrait>(
    wallet_id: i32,
    txn: &C,
  ) -> Result<i64, SumidaError> {
    Ok(Self::lock_wallet(wallet_id, txn).await?.balance)
  }

  /// The reservation, read again under its row lock once its wallet is locked.
  async fn lock_reservation<C: ConnectionTrait>(
    reservation_id: i32,
    txn: &C,
  ) -> Result<(LockedWallet, entity::membership::credit_reservations::Model), SumidaError> {
    let missing = || SumidaError::NoData("CreditReservation".into(), reservation_id.to_string());
    let res = CreditReservations::find_by_id(reservation_id).one(txn).await?.ok_or_else(missing)?;
    let wallet = Self::lock_wallet(res.wallet_id, txn).await?;
    let res = CreditReservations::find_by_id(reservation_id)
      .lock_exclusive()
      .one(txn)
      .await?
      .ok_or_else(missing)?;
    Ok((wallet, res))
  }

  /// A step that moves a hold must find it held and not yet expired.
  fn ensure_live(res: &entity::membership::credit_reservations::Model) -> Result<(), SumidaError> {
    if res.status != ReservationStatus::Held {
      return Err(SumidaError::BadRequest("reservation is not held".into()));
    }
    if res.expires_at.is_some_and(|at| at <= Utc::now()) {
      return Err(SumidaError::ReservationExpired(res.id));
    }
    Ok(())
  }

  async fn reservation_result<C: ConnectionTrait>(
    wallet: &LockedWallet,
    res: &entity::membership::credit_reservations::Model,
    txn: &C,
  ) -> Result<ReservationResult, SumidaError> {
    Ok(ReservationResult {
      reservation_id: res.id,
      wallet_id:      wallet.id,
      amount:         res.amount,
      status:         res.status.clone(),
      expires_at:     res.expires_at,
      spendable:      Self::spendable(wallet, txn).await?,
    })
  }

  /// Who owns the wallet a reservation holds against, for authorizing a step on it.
  async fn reservation_owner<C: ConnectionTrait>(
    reservation_id: i32,
    db: &C,
  ) -> Result<(OwnerKind, i32), SumidaError> {
    let Some(res) = CreditReservations::find_by_id(reservation_id).one(db).await? else {
      return Err(SumidaError::NoData("CreditReservation".into(), reservation_id.to_string()));
    };
    let Some(wallet) = CreditWallets::find_by_id(res.wallet_id).one(db).await? else {
      return Err(SumidaError::NoData("CreditWallet".into(), res.wallet_id.to_string()));
    };
    Ok((wallet.owner_kind, wallet.owner_id))
  }

  // ---- core steps, authorized by the caller ----

  #[allow(clippy::too_many_arguments)]
  pub(crate) async fn debit_in_txn<C: ConnectionTrait>(
    kind: CreditTxKind,
    owner_kind: &OwnerKind,
    owner_id: i32,
    amount: i64,
    action: Option<&str>,
    reference_code: Option<&str>,
    idempotency_key: Option<&str>,
    subscription_id: Option<i32>,
    txn: &C,
  ) -> Result<DebitOutcome, SumidaError> {
    if amount < 0 {
      return Err(SumidaError::BadRequest("amount must be non-negative".into()));
    }

    let Some(mut wallet) = Self::lock_owner_wallet(owner_kind, owner_id, txn).await? else {
      if amount == 0 {
        return Ok(DebitOutcome {
          wallet_id: 0,
          debited:   0,
          balance:   0,
          debt:      0,
        });
      }
      return Err(SumidaError::InsufficientCredits {
        required:  amount,
        available: 0,
      });
    };

    // Idempotency: a prior ledger row with the same key on this wallet → return as-is.
    if let Some(prev) = Self::entry_by_key(wallet.id, idempotency_key, txn).await? {
      return Ok(DebitOutcome {
        wallet_id: wallet.id,
        debited:   -prev.amount,
        balance:   wallet.balance,
        debt:      wallet.debt,
      });
    }

    if amount == 0 {
      return Ok(DebitOutcome {
        wallet_id: wallet.id,
        debited:   0,
        balance:   wallet.balance,
        debt:      wallet.debt,
      });
    }

    let spendable = Self::spendable(&wallet, txn).await?;
    if spendable < amount {
      return Err(SumidaError::InsufficientCredits {
        required:  amount,
        available: spendable,
      });
    }

    Self::take_from_lots(wallet.id, amount, txn).await?;
    wallet.balance -= amount;
    Self::store_wallet(&wallet, txn).await?;
    Self::record(
      &wallet,
      Entry {
        amount: -amount,
        subscription_id,
        action,
        reference_code,
        idempotency_key,
        ..Entry::of(kind)
      },
      txn,
    )
    .await?;

    Ok(DebitOutcome {
      wallet_id: wallet.id,
      debited:   amount,
      balance:   wallet.balance,
      debt:      wallet.debt,
    })
  }

  /// Consume credits: the cost's amount, or its action's fixed cost.
  /// Used by both the `/credits/consume` endpoint and the entitlement gate.
  pub async fn consume_in_txn<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    cost: Cost<'_>,
    idempotency_key: Option<&str>,
    subscription_id: Option<i32>,
    txn: &C,
  ) -> Result<ConsumeOutcome, SumidaError> {
    let amount = cost.resolve(txn).await?;
    let out = Self::debit_in_txn(
      CreditTxKind::Consume,
      owner_kind,
      owner_id,
      amount,
      cost.action,
      None,
      idempotency_key,
      subscription_id,
      txn,
    )
    .await?;

    Ok(ConsumeOutcome {
      wallet_id: out.wallet_id,
      consumed:  out.debited,
      balance:   out.balance,
    })
  }

  /// Put a lot into a wallet and repay its debt from it. Returns the wallet as it then stands.
  #[allow(clippy::too_many_arguments)]
  pub(crate) async fn grant_in_txn<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    amount: i64,
    plan_code: Option<&str>,
    expiry_at: Option<DateTimeUtc>,
    reference_code: Option<&str>,
    idempotency_key: Option<&str>,
    txn: &C,
  ) -> Result<GrantResult, SumidaError> {
    if amount <= 0 {
      return Err(SumidaError::BadRequest("grant amount must be positive".into()));
    }

    let wallet_id = Self::ensure_wallet_id(owner_kind, owner_id, txn).await?;
    let mut wallet = Self::lock_wallet(wallet_id, txn).await?;

    if let Some(prev) = Self::entry_by_key(wallet.id, idempotency_key, txn).await? {
      return Ok(GrantResult {
        wallet_id: wallet.id,
        granted:   prev.amount,
        balance:   wallet.balance,
        debt:      wallet.debt,
      });
    }

    let lot = CreditGrants::insert(CreditGrantsActiveModel {
      wallet_id: Set(wallet_id),
      source_plan_code: Set(plan_code.map(|s| s.to_string())),
      amount_granted: Set(amount),
      amount_remaining: Set(amount),
      expiry_at: Set(expiry_at),
      reference_code: Set(reference_code.map(|s| s.to_string())),
      ..Default::default()
    })
    .exec_with_returning(txn)
    .await?;

    wallet.balance += amount;
    Self::record(
      &wallet,
      Entry {
        amount,
        grant_id: Some(lot.id),
        reference_code,
        idempotency_key,
        ..Entry::of(CreditTxKind::Grant)
      },
      txn,
    )
    .await?;
    Self::settle_debt(&mut wallet, txn).await?;
    Self::store_wallet(&wallet, txn).await?;

    // Clear the low-balance marker once a top-up lifts the balance back above the
    // configured threshold, so a later drop can alert again.
    let clear_low_marker = settings::get()
      .membership
      .credit_low_balance_threshold
      .is_some_and(|t| wallet.balance > t);
    if clear_low_marker {
      CreditWallets::update(CreditWalletsActiveModel {
        id: Unchanged(wallet_id),
        low_balance_notified_at: Set(None),
        ..Default::default()
      })
      .exec(txn)
      .await?;
    }

    Ok(GrantResult {
      wallet_id,
      granted: amount,
      balance: wallet.balance,
      debt: wallet.debt,
    })
  }

  /// Place a soft hold without spending lots. Reduces spendable until captured, released or
  /// expired. Idempotent per (wallet, key).
  pub async fn reserve_in_txn<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    req: HoldRequest<'_>,
    txn: &C,
  ) -> Result<ReservationResult, SumidaError> {
    let Some(wallet) = Self::lock_owner_wallet(owner_kind, owner_id, txn).await? else {
      return Err(SumidaError::InsufficientCredits {
        required:  req.cost.resolve(txn).await?,
        available: 0,
      });
    };

    // Idempotency: same key on this wallet → return the existing reservation.
    if let Some(key) = req.idempotency_key
      && let Some(prev) = CreditReservations::find()
        .filter(CreditReservationsColumn::WalletId.eq(wallet.id))
        .filter(CreditReservationsColumn::IdempotencyKey.eq(key))
        .one(txn)
        .await?
    {
      return Self::reservation_result(&wallet, &prev, txn).await;
    }

    let amount = req.cost.resolve(txn).await?;
    if amount <= 0 {
      return Err(SumidaError::BadRequest("a hold takes a positive amount".into()));
    }
    let spendable = Self::spendable(&wallet, txn).await?;
    if spendable < amount {
      return Err(SumidaError::InsufficientCredits {
        required:  amount,
        available: spendable,
      });
    }

    let res = CreditReservations::insert(CreditReservationsActiveModel {
      wallet_id: Set(wallet.id),
      amount: Set(amount),
      status: Set(ReservationStatus::Held),
      action: Set(req.cost.action.map(str::to_string)),
      reference_code: Set(req.reference_code.map(str::to_string)),
      idempotency_key: Set(req.idempotency_key.map(str::to_string)),
      expires_at: Set(Some(hold_expiry(req.expires_in_secs)?)),
      ..Default::default()
    })
    .exec_with_returning(txn)
    .await?;
    Self::record(
      &wallet,
      Entry {
        action: req.cost.action,
        reference_code: req.reference_code,
        ..Entry::of(CreditTxKind::Reserve)
      },
      txn,
    )
    .await?;

    Self::reservation_result(&wallet, &res, txn).await
  }

  /// Grow a live hold by `amount` and restart its expiry. Refused with 402 when the wallet
  /// cannot spare it. Idempotent per (wallet, key).
  pub async fn extend_in_txn<C: ConnectionTrait>(
    reservation_id: i32,
    amount: i64,
    idempotency_key: Option<&str>,
    expires_in_secs: Option<i64>,
    txn: &C,
  ) -> Result<ReservationResult, SumidaError> {
    let (wallet, res) = Self::lock_reservation(reservation_id, txn).await?;
    if Self::entry_by_key(wallet.id, idempotency_key, txn).await?.is_some() {
      return Self::reservation_result(&wallet, &res, txn).await;
    }
    Self::ensure_live(&res)?;
    if amount <= 0 {
      return Err(SumidaError::BadRequest("an extension takes a positive amount".into()));
    }
    let spendable = Self::spendable(&wallet, txn).await?;
    if spendable < amount {
      return Err(SumidaError::InsufficientCredits {
        required:  amount,
        available: spendable,
      });
    }

    let res = CreditReservations::update(CreditReservationsActiveModel {
      id: Unchanged(res.id),
      amount: Set(res.amount + amount),
      expires_at: Set(Some(hold_expiry(expires_in_secs)?)),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Self::record(
      &wallet,
      Entry {
        action: res.action.as_deref(),
        reference_code: res.reference_code.as_deref(),
        idempotency_key,
        ..Entry::of(CreditTxKind::Extend)
      },
      txn,
    )
    .await?;

    Self::reservation_result(&wallet, &res, txn).await
  }

  /// Spend a live hold at the amount actually used, which may exceed the hold. What the wallet
  /// cannot cover becomes debt up to `debt_limit`; past it the capture is refused with 402 and
  /// nothing changes. Idempotent per (wallet, key).
  pub async fn capture_in_txn<C: ConnectionTrait>(
    reservation_id: i32,
    req: CaptureRequest<'_>,
    txn: &C,
  ) -> Result<ConsumeResult, SumidaError> {
    let (mut wallet, res) = Self::lock_reservation(reservation_id, txn).await?;
    if let Some(prev) = Self::entry_by_key(wallet.id, req.idempotency_key, txn).await? {
      return Ok(ConsumeResult {
        wallet_id:     wallet.id,
        consumed:      -prev.amount,
        debt_incurred: prev.debt_change,
        balance:       wallet.balance,
        debt:          wallet.debt,
      });
    }
    Self::ensure_live(&res)?;
    let actual = req.amount.unwrap_or(res.amount);
    if actual < 0 {
      return Err(SumidaError::BadRequest("amount must be non-negative".into()));
    }

    // Mark captured first so the hold no longer counts against what the capture may spend.
    CreditReservations::update(CreditReservationsActiveModel {
      id: Unchanged(res.id),
      amount: Set(actual),
      status: Set(ReservationStatus::Captured),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    let spendable = Self::spendable(&wallet, txn).await?;
    let paid = actual.min(spendable);
    let shortfall = actual - paid;
    if shortfall > req.debt_limit.max(0) {
      return Err(SumidaError::InsufficientCredits {
        required:  actual,
        available: spendable,
      });
    }

    Self::take_from_lots(wallet.id, paid, txn).await?;
    wallet.balance -= paid;
    wallet.debt += shortfall;
    Self::store_wallet(&wallet, txn).await?;
    Self::record(
      &wallet,
      Entry {
        amount: -paid,
        debt_change: shortfall,
        action: res.action.as_deref(),
        reference_code: res.reference_code.as_deref(),
        idempotency_key: req.idempotency_key,
        ..Entry::of(CreditTxKind::Capture)
      },
      txn,
    )
    .await?;

    Ok(ConsumeResult {
      wallet_id:     wallet.id,
      consumed:      paid,
      debt_incurred: shortfall,
      balance:       wallet.balance,
      debt:          wallet.debt,
    })
  }

  /// Drop a held reservation without spending it, expired or not, and repay debt from what it
  /// frees. Idempotent per (wallet, key).
  pub async fn release_in_txn<C: ConnectionTrait>(
    reservation_id: i32,
    idempotency_key: Option<&str>,
    txn: &C,
  ) -> Result<ReservationResult, SumidaError> {
    let (mut wallet, res) = Self::lock_reservation(reservation_id, txn).await?;
    if Self::entry_by_key(wallet.id, idempotency_key, txn).await?.is_some() {
      return Self::reservation_result(&wallet, &res, txn).await;
    }
    if res.status != ReservationStatus::Held {
      return Err(SumidaError::BadRequest("reservation is not held".into()));
    }

    let res = CreditReservations::update(CreditReservationsActiveModel {
      id: Unchanged(res.id),
      status: Set(ReservationStatus::Released),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Self::record(
      &wallet,
      Entry {
        action: res.action.as_deref(),
        reference_code: res.reference_code.as_deref(),
        idempotency_key,
        ..Entry::of(CreditTxKind::Release)
      },
      txn,
    )
    .await?;
    if Self::settle_debt(&mut wallet, txn).await? {
      Self::store_wallet(&wallet, txn).await?;
    }

    Self::reservation_result(&wallet, &res, txn).await
  }

  // ---- public API ----

  pub async fn balance(
    &self,
    owner_kind: OwnerKind,
    owner_id: Option<i32>,
  ) -> Result<WalletBalance, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &owner_kind, owner_id);
    self.authorize_use(&owner_kind, owner_id, self.state.db()).await?;

    let txn = self.state.db().begin().await?;
    let result = match Self::lock_owner_wallet(&owner_kind, owner_id, &txn).await? {
      Some(wallet) => {
        let reserved = Self::held_reserved(wallet.id, &txn).await?;
        WalletBalance {
          wallet_id: wallet.id,
          spendable: (wallet.balance - reserved - wallet.debt).max(0),
          owner_kind: wallet.owner_kind,
          owner_id: wallet.owner_id,
          balance: wallet.balance,
          reserved,
          debt: wallet.debt,
        }
      }
      None => WalletBalance {
        wallet_id: 0,
        owner_kind,
        owner_id,
        balance: 0,
        reserved: 0,
        debt: 0,
        spendable: 0,
      },
    };
    txn.commit().await?;
    Ok(result)
  }

  pub async fn grant(&self, data: GrantCredit) -> Result<GrantResult, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &data.owner_kind, data.owner_id);
    let expiry_at = data.expiry_at.or_else(|| {
      settings::get()
        .membership
        .credit_default_expiry_days
        .map(|d| Utc::now() + chrono::Duration::days(d as i64))
    });

    let txn = self.state.db().begin().await?;
    self.authorize_manage(&owner_kind, owner_id, &txn).await?;
    let result = Self::grant_in_txn(
      &owner_kind,
      owner_id,
      data.amount,
      data.plan_code.as_deref(),
      expiry_at,
      data.reference_code.as_deref(),
      data.idempotency_key.as_deref(),
      &txn,
    )
    .await?;
    txn.commit().await?;

    let emails = recipient_emails(&owner_kind, owner_id, self.state.db()).await;
    notify_all::<S, _, _>(
      "credit-purchased-receipt",
      &emails,
      serde_json::json!({ "amount": data.amount, "balance": result.balance }),
      self.state.db(),
    )
    .await;

    Ok(result)
  }

  pub async fn consume(&self, data: ConsumeCredit) -> Result<ConsumeResult, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &data.owner_kind, data.owner_id);
    let txn = self.state.db().begin().await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let cost = Cost {
      action:    data.action.as_deref(),
      amount:    data.amount,
      plan_code: data.plan_code.as_deref(),
    };
    let amount = cost.resolve(&txn).await?;
    let out = Self::debit_in_txn(
      CreditTxKind::Consume,
      &owner_kind,
      owner_id,
      amount,
      cost.action,
      None,
      data.idempotency_key.as_deref(),
      None,
      &txn,
    )
    .await?;
    txn.commit().await?;

    Ok(ConsumeResult {
      wallet_id:     out.wallet_id,
      consumed:      out.debited,
      debt_incurred: 0,
      balance:       out.balance,
      debt:          out.debt,
    })
  }

  // ---- reservations (authorize → extend → capture / release) ----

  pub async fn reserve(&self, data: ReserveCredit) -> Result<ReservationResult, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &data.owner_kind, data.owner_id);
    let txn = self.state.db().begin().await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let req = HoldRequest {
      cost:            Cost {
        action:    data.action.as_deref(),
        amount:    data.amount,
        plan_code: data.plan_code.as_deref(),
      },
      reference_code:  data.reference_code.as_deref(),
      idempotency_key: data.idempotency_key.as_deref(),
      expires_in_secs: data.expires_in_secs,
    };
    let result = Self::reserve_in_txn(&owner_kind, owner_id, req, &txn).await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn extend(
    &self,
    reservation_id: i32,
    data: ExtendReservation,
  ) -> Result<ReservationResult, SumidaError> {
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
    data: CaptureReservation,
  ) -> Result<ConsumeResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let (owner_kind, owner_id) = Self::reservation_owner(reservation_id, &txn).await?;
    let debt_limit = data.debt_limit.unwrap_or(0);
    self.authorize_capture(&owner_kind, owner_id, debt_limit, &txn).await?;
    let req = CaptureRequest {
      amount: data.amount,
      debt_limit,
      idempotency_key: data.idempotency_key.as_deref(),
    };
    let result = Self::capture_in_txn(reservation_id, req, &txn).await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn release(
    &self,
    reservation_id: i32,
    data: ReleaseReservation,
  ) -> Result<ReservationResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let (owner_kind, owner_id) = Self::reservation_owner(reservation_id, &txn).await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;
    let result = Self::release_in_txn(reservation_id, data.idempotency_key.as_deref(), &txn).await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn refund(
    &self,
    owner_kind: OwnerKind,
    owner_id: Option<i32>,
    amount: i64,
    reference_code: Option<String>,
    idempotency_key: Option<String>,
  ) -> Result<ConsumeResult, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &owner_kind, owner_id);
    let txn = self.state.db().begin().await?;
    self.authorize_manage(&owner_kind, owner_id, &txn).await?;

    let out = Self::debit_in_txn(
      CreditTxKind::Refund,
      &owner_kind,
      owner_id,
      amount,
      None,
      reference_code.as_deref(),
      idempotency_key.as_deref(),
      None,
      &txn,
    )
    .await
    .map_err(|e| match e {
      SumidaError::InsufficientCredits { available, .. } => {
        SumidaError::RefundWouldExceedBalance(amount, available)
      }
      other => other,
    })?;
    txn.commit().await?;

    Ok(ConsumeResult {
      wallet_id:     out.wallet_id,
      consumed:      out.debited,
      debt_incurred: 0,
      balance:       out.balance,
      debt:          out.debt,
    })
  }

  pub async fn transactions(
    &self,
    filter: CreditTxFilter,
    pagination: Pagination,
  ) -> Result<(Vec<CreditTxItem>, u64), SumidaError> {
    use entity::membership::credit_transactions::Column;

    let db = self.state.db();
    let Some(wallet_id) = filter.wallet_id else {
      return Err(SumidaError::BadRequest("walletId is required".into()));
    };

    // Owner-scope: only the wallet's owner (or admin / org manager) may read its ledger.
    let Some(wallet) = CreditWallets::find_by_id(wallet_id).one(db).await? else {
      return Ok((vec![], 0));
    };
    self.authorize_use(&wallet.owner_kind, wallet.owner_id, db).await?;

    let mut query = CreditTransactions::find()
      .filter(Column::WalletId.eq(wallet_id))
      .apply_if(filter.kind.clone(), |q, v| q.filter(Column::Kind.eq(v)));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "createdAt" => Column::CreatedAt,
      _ => Column::Id,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<CreditTxItem>().all(db).await?;
    Ok((items, count))
  }

  /// List a wallet's reservations (owner-scoped, like the ledger).
  pub async fn reservations(
    &self,
    filter: ReservationFilter,
    pagination: Pagination,
  ) -> Result<(Vec<ReservationListItem>, u64), SumidaError> {
    let db = self.state.db();
    let Some(wallet_id) = filter.wallet_id else {
      return Err(SumidaError::BadRequest("walletId is required".into()));
    };
    let Some(wallet) = CreditWallets::find_by_id(wallet_id).one(db).await? else {
      return Ok((vec![], 0));
    };
    self.authorize_use(&wallet.owner_kind, wallet.owner_id, db).await?;

    let mut query = CreditReservations::find()
      .filter(CreditReservationsColumn::WalletId.eq(wallet_id))
      .apply_if(filter.status.clone(), |q, v| q.filter(CreditReservationsColumn::Status.eq(v)));
    let count = query.clone().select_only().column(CreditReservationsColumn::Id).count(db).await?;
    query = pagination.order_by(query, |col| match col {
      "createdAt" => CreditReservationsColumn::CreatedAt,
      _ => CreditReservationsColumn::Id,
    });
    query = pagination.paginate(query);
    let items = query.into_model::<ReservationListItem>().all(db).await?;
    Ok((items, count))
  }

  /// Consistency audit: compare the cached `balance` and `debt` against the sums of the
  /// append-only ledger's `amount` and `debt_change`. A divergence is a bug (logged at error
  /// level) — the ledger is authoritative.
  pub async fn audit(&self, wallet_id: i32) -> Result<WalletAudit, SumidaError> {
    let db = self.state.db();
    let Some(wallet) = CreditWallets::find_by_id(wallet_id).one(db).await? else {
      return Err(SumidaError::NoData("CreditWallet".into(), wallet_id.to_string()));
    };
    self.authorize_use(&wallet.owner_kind, wallet.owner_id, db).await?;

    #[derive(FromQueryResult)]
    struct SumRow {
      amount: i64,
      debt:   i64,
    }
    let backend = db.get_database_backend();
    let sums = SumRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT COALESCE(SUM(amount), 0)::bigint AS amount,
                COALESCE(SUM(debt_change), 0)::bigint AS debt
         FROM credit_transactions WHERE wallet_id = $1"#,
      [wallet_id.into()],
    ))
    .one(db)
    .await?
    .unwrap_or(SumRow {
      amount: 0,
      debt:   0,
    });

    let diverged = sums.amount != wallet.balance || sums.debt != wallet.debt;
    if diverged {
      tracing::error!(
        "credit wallet {} divergence: balance cache={} ledger={}, debt cache={} ledger={}",
        wallet_id,
        wallet.balance,
        sums.amount,
        wallet.debt,
        sums.debt
      );
    }
    Ok(WalletAudit {
      wallet_id,
      balance: wallet.balance,
      ledger_sum: sums.amount,
      debt: wallet.debt,
      ledger_debt: sums.debt,
      diverged,
    })
  }
}
