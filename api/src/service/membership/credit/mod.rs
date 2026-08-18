pub mod dto;

use chrono::{Duration, Utc};
use entity::membership::{CreditTxKind, OwnerKind, ReservationStatus};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
  ColumnTrait, Condition, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait,
  QueryFilter, QueryOrder, QuerySelect, QueryTrait, Statement, TransactionTrait,
};

use self::dto::*;
use super::{notify_all, owner_kind_str, recipient_emails, resolve_owner};
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::pagination::Pagination;

/// Outcome of a FIFO debit (consume/refund).
pub struct DebitOutcome {
  pub wallet_id: i32,
  pub debited: i64,
  pub balance: i64,
}

/// Outcome of a consume (for the entitlement gate).
pub struct ConsumeOutcome {
  pub wallet_id: i32,
  pub consumed: i64,
  pub balance: i64,
}

pub struct CreditService<S: SumidaState> {
  state: S,
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

  /// Manage = grant/refund. 최상위 관리자 / 본인 / `organization#manage_credit` 보유자(계층 인지).
  ///
  /// 개인 지갑 갈래에는 대응하는 규칙이 없다 — `organization` 정의만이 credit 층을 갖는다.
  /// 그래서 최상위 관리자 판정은 남되, 세션의 `is_admin` 필드를 직접 읽는 대신
  /// `platform:root#superuser` 로 묻는다. 보안 API 키가 소유자의 관리자 권한을 물려받지 않는
  /// 것이 그 차이다.
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
      OwnerKind::Personal if self.session.profile.id == owner_id => Ok(()),
      OwnerKind::Personal => Err(SumidaError::NoAccessPermission),
      OwnerKind::Organization => {
        // 조상 조직의 부여가 하위 조직에 닿는 것은 `manager^` / `credit_manager^` 가 맡는다.
        // 행마다 재귀 CTE 를 돌리던 `organization_ancestors_subquery` 는 그 닫힘으로 대체됐다.
        Authz::try_check("organization", owner_id, "manage_credit", &self.subject(), db).await
      }
    }
  }

  /// Use = consume/read balance. 최상위 관리자 / 본인 / `organization#view_credit` 보유자.
  ///
  /// 종전의 "같은 조직 구성원이면 통과" 단축로는 사라졌다. 없어진 것이 아니라 규칙 안으로
  /// 들어갔다 — `view_credit = credit_viewer^ | edit_credit | member` 의 `member` 갈래가 바로
  /// 그것이고, 이제 목록·단건 어느 쪽에서 물어도 같은 답이 나온다.
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

  /// Sum of credits currently on hold (reservations in `held` status) for a wallet.
  async fn held_reserved<C: ConnectionTrait>(wallet_id: i32, txn: &C) -> Result<i64, SumidaError> {
    #[derive(FromQueryResult)]
    struct SumRow {
      total: i64,
    }
    let backend = txn.get_database_backend();
    let row = SumRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT COALESCE(SUM(amount), 0)::bigint AS total
         FROM credit_reservations WHERE wallet_id = $1 AND status = 'held'"#,
      [wallet_id.into()],
    ))
    .one(txn)
    .await?;
    Ok(row.map(|r| r.total).unwrap_or(0))
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

  /// Expire any lapsed lots: zero their remaining, write `expire` ledger rows, and
  /// decrement the cached wallet balance. Returns the post-sweep balance. The caller
  /// must already hold the wallet row lock (this runs inside the same transaction).
  async fn sweep_expired<C: ConnectionTrait>(
    wallet_id: i32,
    current_balance: i64,
    txn: &C,
  ) -> Result<i64, SumidaError> {
    let now: DateTimeWithTimeZone = Utc::now().into();
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

  /// Background-maintenance entrypoint: lock a wallet and expire its lapsed lots.
  /// Returns the post-sweep balance. Runs inside the caller's transaction.
  pub(crate) async fn sweep_wallet_in_txn<C: ConnectionTrait>(
    wallet_id: i32,
    txn: &C,
  ) -> Result<i64, SumidaError> {
    let wallet = CreditWallets::find_by_id(wallet_id)
      .lock_exclusive()
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::InternalServerError("wallet vanished".into()))?;
    Self::sweep_expired(wallet.id, wallet.balance, txn).await
  }

  // ---- core debit (consume / refund) ----

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

    let Some(wallet0) = Self::find_wallet(owner_kind, owner_id, txn).await? else {
      if amount == 0 {
        return Ok(DebitOutcome {
          wallet_id: 0,
          debited: 0,
          balance: 0,
        });
      }
      return Err(SumidaError::InsufficientCredits {
        required: amount,
        available: 0,
      });
    };

    // Lock the wallet row to serialize concurrent debits/grants.
    let wallet = CreditWallets::find_by_id(wallet0.id).lock_exclusive().one(txn).await?.ok_or(
      SumidaError::InsufficientCredits {
        required: amount,
        available: 0,
      },
    )?;

    // Expire lapsed lots first so balance & availability are accurate.
    let balance = Self::sweep_expired(wallet.id, wallet.balance, txn).await?;

    // Idempotency: a prior ledger row with the same key on this wallet → return as-is.
    if let Some(key) = idempotency_key {
      if let Some(prev) = CreditTransactions::find()
        .filter(CreditTransactionsColumn::WalletId.eq(wallet.id))
        .filter(CreditTransactionsColumn::IdempotencyKey.eq(key))
        .one(txn)
        .await?
      {
        return Ok(DebitOutcome {
          wallet_id: wallet.id,
          debited: -prev.amount,
          balance,
        });
      }
    }

    if amount == 0 {
      return Ok(DebitOutcome {
        wallet_id: wallet.id,
        debited: 0,
        balance,
      });
    }

    // Lock non-expired lots in FIFO order (expiry NULLS LAST, then created_at).
    let now: DateTimeWithTimeZone = Utc::now().into();
    let lots = CreditGrants::find()
      .filter(CreditGrantsColumn::WalletId.eq(wallet.id))
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

    // Spendable excludes credits currently on hold by reservations. A capture
    // marks its own reservation captured before debiting, so it is not counted.
    let available: i64 = lots.iter().map(|l| l.amount_remaining).sum();
    let reserved = Self::held_reserved(wallet.id, txn).await?;
    let spendable = available - reserved;
    if spendable < amount {
      return Err(SumidaError::InsufficientCredits {
        required: amount,
        available: spendable.max(0),
      });
    }

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

    let new_balance = balance - amount;
    CreditWallets::update(CreditWalletsActiveModel {
      id: Unchanged(wallet.id),
      balance: Set(new_balance),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    CreditTransactions::insert(CreditTransactionsActiveModel {
      wallet_id: Set(wallet.id),
      grant_id: Set(None),
      subscription_id: Set(subscription_id),
      kind: Set(kind),
      amount: Set(-amount),
      action: Set(action.map(|s| s.to_string())),
      reference_code: Set(reference_code.map(|s| s.to_string())),
      idempotency_key: Set(idempotency_key.map(|s| s.to_string())),
      balance_after: Set(new_balance),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    Ok(DebitOutcome {
      wallet_id: wallet.id,
      debited: amount,
      balance: new_balance,
    })
  }

  /// Consume credits for an action (resolves the per-action cost first).
  /// Used by both the `/credits/consume` endpoint and the entitlement gate.
  #[allow(clippy::too_many_arguments)]
  pub async fn consume_in_txn<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    plan_code: Option<&str>,
    idempotency_key: Option<&str>,
    subscription_id: Option<i32>,
    txn: &C,
  ) -> Result<ConsumeOutcome, SumidaError> {
    let cost = Self::resolve_cost(action, plan_code, txn)
      .await?
      .ok_or_else(|| SumidaError::MembershipActionNotAllowed(action.to_string()))?;

    let out = Self::debit_in_txn(
      CreditTxKind::Consume,
      owner_kind,
      owner_id,
      cost,
      Some(action),
      None,
      idempotency_key,
      subscription_id,
      txn,
    )
    .await?;

    Ok(ConsumeOutcome {
      wallet_id: out.wallet_id,
      consumed: out.debited,
      balance: out.balance,
    })
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) async fn grant_in_txn<C: ConnectionTrait>(
    owner_kind: &OwnerKind,
    owner_id: i32,
    amount: i64,
    plan_code: Option<&str>,
    expiry_at: Option<DateTimeWithTimeZone>,
    reference_code: Option<&str>,
    idempotency_key: Option<&str>,
    txn: &C,
  ) -> Result<GrantResult, SumidaError> {
    if amount <= 0 {
      return Err(SumidaError::BadRequest("grant amount must be positive".into()));
    }

    let wallet_id = Self::ensure_wallet_id(owner_kind, owner_id, txn).await?;
    let wallet = CreditWallets::find_by_id(wallet_id)
      .lock_exclusive()
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::InternalServerError("wallet vanished".into()))?;

    let balance = Self::sweep_expired(wallet.id, wallet.balance, txn).await?;

    if let Some(key) = idempotency_key {
      if let Some(prev) = CreditTransactions::find()
        .filter(CreditTransactionsColumn::WalletId.eq(wallet.id))
        .filter(CreditTransactionsColumn::IdempotencyKey.eq(key))
        .one(txn)
        .await?
      {
        return Ok(GrantResult {
          wallet_id: wallet.id,
          granted: prev.amount,
          balance,
        });
      }
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

    let new_balance = balance + amount;
    // Clear the low-balance marker once a top-up lifts the balance back above the
    // configured threshold, so a later drop can alert again.
    let clear_low_marker = settings::get()
      .membership
      .credit_low_balance_threshold
      .map(|t| new_balance > t)
      .unwrap_or(false);
    let mut wallet_update = CreditWalletsActiveModel {
      id: Unchanged(wallet_id),
      balance: Set(new_balance),
      ..Default::default()
    };
    if clear_low_marker {
      wallet_update.low_balance_notified_at = Set(None);
    }
    CreditWallets::update(wallet_update).exec(txn).await?;

    CreditTransactions::insert(CreditTransactionsActiveModel {
      wallet_id: Set(wallet_id),
      grant_id: Set(Some(lot.id)),
      subscription_id: Set(None),
      kind: Set(CreditTxKind::Grant),
      amount: Set(amount),
      action: Set(None),
      reference_code: Set(reference_code.map(|s| s.to_string())),
      idempotency_key: Set(idempotency_key.map(|s| s.to_string())),
      balance_after: Set(new_balance),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    Ok(GrantResult {
      wallet_id,
      granted: amount,
      balance: new_balance,
    })
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
    let result = match Self::find_wallet(&owner_kind, owner_id, &txn).await? {
      Some(w) => {
        // Lock + sweep so the reported balance excludes lapsed lots and self-heals the cache.
        let wallet = CreditWallets::find_by_id(w.id)
          .lock_exclusive()
          .one(&txn)
          .await?
          .ok_or_else(|| SumidaError::InternalServerError("wallet vanished".into()))?;
        let balance = Self::sweep_expired(wallet.id, wallet.balance, &txn).await?;
        let reserved = Self::held_reserved(wallet.id, &txn).await?;
        WalletBalance {
          wallet_id: wallet.id,
          owner_kind: wallet.owner_kind,
          owner_id: wallet.owner_id,
          balance,
          reserved,
          spendable: balance - reserved,
        }
      }
      None => WalletBalance {
        wallet_id: 0,
        owner_kind,
        owner_id,
        balance: 0,
        reserved: 0,
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
        .map(|d| (Utc::now() + chrono::Duration::days(d as i64)).into())
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
    let out = self
      .consume_in_txn(
        &owner_kind,
        owner_id,
        &data.action,
        data.plan_code.as_deref(),
        data.idempotency_key.as_deref(),
        None,
        &txn,
      )
      .await?;
    txn.commit().await?;

    Ok(ConsumeResult {
      wallet_id: out.wallet_id,
      consumed: out.consumed,
      balance: out.balance,
    })
  }

  // ---- reservations (authorize → capture / release) ----

  /// Place a soft hold for `action`'s cost without spending lots. Reduces
  /// spendable balance until captured/released. Idempotent per (wallet, key).
  pub async fn reserve(&self, data: ReserveCredit) -> Result<ReservationResult, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &data.owner_kind, data.owner_id);
    let txn = self.state.db().begin().await?;
    self.authorize_use(&owner_kind, owner_id, &txn).await?;

    let Some(wallet0) = Self::find_wallet(&owner_kind, owner_id, &txn).await? else {
      return Err(SumidaError::InsufficientCredits {
        required: 0,
        available: 0,
      });
    };
    let wallet = CreditWallets::find_by_id(wallet0.id).lock_exclusive().one(&txn).await?.ok_or(
      SumidaError::InsufficientCredits {
        required: 0,
        available: 0,
      },
    )?;
    let balance = Self::sweep_expired(wallet.id, wallet.balance, &txn).await?;

    // Idempotency: same key on this wallet → return the existing reservation.
    if let Some(key) = &data.idempotency_key {
      if let Some(prev) = CreditReservations::find()
        .filter(CreditReservationsColumn::WalletId.eq(wallet.id))
        .filter(CreditReservationsColumn::IdempotencyKey.eq(key.clone()))
        .one(&txn)
        .await?
      {
        let reserved = Self::held_reserved(wallet.id, &txn).await?;
        txn.commit().await?;
        return Ok(ReservationResult {
          reservation_id: prev.id,
          wallet_id: wallet.id,
          amount: prev.amount,
          status: prev.status,
          spendable: balance - reserved,
        });
      }
    }

    let cost = Self::resolve_cost(&data.action, data.plan_code.as_deref(), &txn)
      .await?
      .ok_or_else(|| SumidaError::MembershipActionNotAllowed(data.action.clone()))?;

    let reserved = Self::held_reserved(wallet.id, &txn).await?;
    let spendable = balance - reserved;
    if spendable < cost {
      return Err(SumidaError::InsufficientCredits {
        required: cost,
        available: spendable.max(0),
      });
    }

    let expires_at = data.expires_in_secs.map(|s| (Utc::now() + Duration::seconds(s)).into());
    let res = CreditReservations::insert(CreditReservationsActiveModel {
      wallet_id: Set(wallet.id),
      amount: Set(cost),
      status: Set(ReservationStatus::Held),
      action: Set(Some(data.action.clone())),
      reference_code: Set(data.reference_code.clone()),
      idempotency_key: Set(data.idempotency_key.clone()),
      expires_at: Set(expires_at),
      ..Default::default()
    })
    .exec_with_returning(&txn)
    .await?;

    txn.commit().await?;
    Ok(ReservationResult {
      reservation_id: res.id,
      wallet_id: wallet.id,
      amount: cost,
      status: ReservationStatus::Held,
      spendable: spendable - cost,
    })
  }

  /// Confirm a held reservation into a real spend: debit the lots and mark it
  /// `captured`. No-op-safe — a non-held reservation is rejected.
  pub async fn capture(&self, reservation_id: i32) -> Result<ConsumeResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(res) = CreditReservations::find_by_id(reservation_id).one(&txn).await? else {
      return Err(SumidaError::NoData("CreditReservation".into(), reservation_id.to_string()));
    };
    let Some(wallet) = CreditWallets::find_by_id(res.wallet_id).one(&txn).await? else {
      return Err(SumidaError::NoData("CreditWallet".into(), res.wallet_id.to_string()));
    };
    self.authorize_use(&wallet.owner_kind, wallet.owner_id, &txn).await?;

    if res.status != ReservationStatus::Held {
      return Err(SumidaError::BadRequest("reservation is not held".into()));
    }

    // Mark captured first so the hold is excluded from the debit's spendable check.
    CreditReservations::update(CreditReservationsActiveModel {
      id: Unchanged(res.id),
      status: Set(ReservationStatus::Captured),
      ..Default::default()
    })
    .exec(&txn)
    .await?;

    let out = Self::debit_in_txn(
      CreditTxKind::Capture,
      &wallet.owner_kind,
      wallet.owner_id,
      res.amount,
      res.action.as_deref(),
      res.reference_code.as_deref(),
      None,
      None,
      &txn,
    )
    .await?;

    txn.commit().await?;
    Ok(ConsumeResult {
      wallet_id: out.wallet_id,
      consumed: out.debited,
      balance: out.balance,
    })
  }

  /// Cancel a held reservation without spending. Rejected if not held.
  pub async fn release(&self, reservation_id: i32) -> Result<ReservationResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(res) = CreditReservations::find_by_id(reservation_id).one(&txn).await? else {
      return Err(SumidaError::NoData("CreditReservation".into(), reservation_id.to_string()));
    };
    let Some(wallet) = CreditWallets::find_by_id(res.wallet_id).one(&txn).await? else {
      return Err(SumidaError::NoData("CreditWallet".into(), res.wallet_id.to_string()));
    };
    self.authorize_use(&wallet.owner_kind, wallet.owner_id, &txn).await?;

    if res.status != ReservationStatus::Held {
      return Err(SumidaError::BadRequest("reservation is not held".into()));
    }

    CreditReservations::update(CreditReservationsActiveModel {
      id: Unchanged(res.id),
      status: Set(ReservationStatus::Released),
      ..Default::default()
    })
    .exec(&txn)
    .await?;

    let reserved = Self::held_reserved(wallet.id, &txn).await?;
    txn.commit().await?;
    Ok(ReservationResult {
      reservation_id: res.id,
      wallet_id: wallet.id,
      amount: res.amount,
      status: ReservationStatus::Released,
      spendable: wallet.balance - reserved,
    })
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
      wallet_id: out.wallet_id,
      consumed: out.debited,
      balance: out.balance,
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

  /// Consistency audit: compare the cached `balance` against the sum of the
  /// append-only ledger. A divergence is a bug (logged at error level) — the
  /// ledger is authoritative.
  pub async fn audit(&self, wallet_id: i32) -> Result<WalletAudit, SumidaError> {
    let db = self.state.db();
    let Some(wallet) = CreditWallets::find_by_id(wallet_id).one(db).await? else {
      return Err(SumidaError::NoData("CreditWallet".into(), wallet_id.to_string()));
    };
    self.authorize_use(&wallet.owner_kind, wallet.owner_id, db).await?;

    #[derive(FromQueryResult)]
    struct SumRow {
      total: i64,
    }
    let backend = db.get_database_backend();
    let ledger_sum = SumRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT COALESCE(SUM(amount), 0)::bigint AS total
         FROM credit_transactions WHERE wallet_id = $1"#,
      [wallet_id.into()],
    ))
    .one(db)
    .await?
    .map(|r| r.total)
    .unwrap_or(0);

    let diverged = ledger_sum != wallet.balance;
    if diverged {
      tracing::error!(
        "credit wallet {} balance divergence: cache={} ledger={}",
        wallet_id,
        wallet.balance,
        ledger_sum
      );
    }
    Ok(WalletAudit {
      wallet_id,
      balance: wallet.balance,
      ledger_sum,
      diverged,
    })
  }
}
