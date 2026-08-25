use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{CREDIT_TAG, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/credits";

const BALANCE: &str = "/balance";
const TRANSACTIONS: &str = "/transactions";
const GRANT: &str = "/grant";
const CONSUME: &str = "/consume";
const REFUND: &str = "/refund";
const RESERVE: &str = "/reserve";
const RESERVATIONS: &str = "/reservations";
const CAPTURE: &str = "/reservations/{id}/capture";
const RELEASE: &str = "/reservations/{id}/release";
const AUDIT: &str = "/wallets/{id}/audit";

/// Who these routes let through, repeated on each of them because a caller reading one operation
/// has no reason to look at the others.
const OWNERSHIP: &str = "Being signed in is all the route itself asks for. Whose wallet may be \
reached is settled inside the handler: its owner may, and so may an administrator of the \
organization that owns it. `ownerKind` left out means the caller's own personal wallet.";

/// What a repeated request does. Said on every route that moves credits, because a network retry on
/// one of these is the ordinary case rather than the exception.
const IDEMPOTENT: &str = "`idempotencyKey` makes a retry safe: the same key seen twice answers with \
what the first call did instead of doing it again. Without one, a retried request moves credits a \
second time.";

#[derive(Clone, Debug, Default)]
pub struct CreditController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> CreditController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }
}

/// What a wallet holds, and how much of it can be spent right now.
///
/// `balance` is everything not yet expired; `reserved` is what outstanding holds have taken out of
/// reach; `spendable` is the difference, and is the number that decides whether an action can go
/// ahead.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = BALANCE,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_balance",
  description = OWNERSHIP,
  params(OwnerQuery),
  responses(
    (status = 200, description = "The wallet, its balance, its holds and what is left", body = WalletBalance),
    Unreadable,
    Refused,
  ),
))]
async fn balance<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(owner): Query<OwnerQuery>,
) -> Result<WalletBalance, SumidaError> {
  CreditService::new(state, session).balance(owner.owner_kind, owner.owner_id).await
}

/// The ledger — every movement in or out, with the running balance it left behind.
///
/// This is the authoritative record; the wallet's own `balance` is a cache of it. Paging applies,
/// but there is no condition tree here and no `filters` route: `walletId` and `kind` are the whole
/// of what narrows it.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = TRANSACTIONS,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_transactions",
  description = OWNERSHIP,
  params(CreditTxFilter, Pagination),
  responses(
    (status = 200, description = "One page of ledger entries, and how many there are behind it", body = List<CreditTxItem>),
    Unreadable,
    Refused,
  ),
))]
async fn transactions<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<CreditTxFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<CreditTxItem>, SumidaError> {
  Ok(CreditService::new(state, session).transactions(filter, pagination).await?.into())
}

/// Put credits into a wallet.
///
/// They arrive as a lot with its own expiry, and spending draws from the lot that expires first. A
/// lot with no `expiryAt` never lapses.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = GRANT,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_grant",
  description = IDEMPOTENT,
  request_body = GrantCredit,
  responses(
    (status = 200, description = "What was added and what the wallet holds now", body = GrantResult),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn grant<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<GrantCredit>,
) -> Result<GrantResult, SumidaError> {
  CreditService::new(state, session).grant(body).await
}

/// Spend credits on an action, at whatever that action costs under the plan.
///
/// It is spent outright — nothing here to capture or release afterwards. Too little in the wallet
/// is refused with `402`, not with a partial spend.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CONSUME,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_consume",
  description = IDEMPOTENT,
  request_body = ConsumeCredit,
  responses(
    (status = 200, description = "What was spent and what the wallet holds now", body = ConsumeResult),
    (status = 402, description = "The wallet does not hold enough. Nothing was spent", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn consume<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<ConsumeCredit>,
) -> Result<ConsumeResult, SumidaError> {
  CreditService::new(state, session).consume(body).await
}

/// Put spent credits back.
///
/// A refund over what the wallet has actually spent is refused rather than trimmed, so the ledger
/// cannot be talked into a balance it never held.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = REFUND,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_refund",
  description = IDEMPOTENT,
  request_body = RefundCredit,
  responses(
    (status = 200, description = "What was returned and what the wallet holds now", body = ConsumeResult),
    (status = 400, description = "The amount is more than the wallet has spent", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn refund<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<RefundCredit>,
) -> Result<ConsumeResult, SumidaError> {
  CreditService::new(state, session)
    .refund(body.owner_kind, body.owner_id, body.amount, body.reference_code, body.idempotency_key)
    .await
}

/// Hold credits aside for an action that has not finished yet.
///
/// Nothing is spent: the amount comes out of `spendable` and stays in `balance` until the hold is
/// captured or released. `expiresInSecs` lets it fall away on its own, which is what keeps a client
/// that walks away from stranding the credits forever.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = RESERVE,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_reserve",
  description = IDEMPOTENT,
  request_body = ReserveCredit,
  responses(
    (status = 200, description = "The hold, and what is left spendable behind it", body = ReservationResult),
    (status = 402, description = "The wallet does not hold enough. Nothing was held", body = ErrorBody),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn reserve<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<ReserveCredit>,
) -> Result<ReservationResult, SumidaError> {
  CreditService::new(state, session).reserve(body).await
}

/// The holds standing against a wallet.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = RESERVATIONS,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_reservations",
  description = OWNERSHIP,
  params(ReservationFilter, Pagination),
  responses(
    (status = 200, description = "One page of holds, and how many there are behind it", body = List<ReservationListItem>),
    Unreadable,
    Refused,
  ),
))]
async fn reservations<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<ReservationFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<ReservationListItem>, SumidaError> {
  Ok(CreditService::new(state, session).reservations(filter, pagination).await?.into())
}

/// Spend a hold. The credits leave the wallet for good and the ledger records the movement.
///
/// The body is not read.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CAPTURE,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_capture",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The hold's identifier")),
  responses(
    (status = 200, description = "What was spent and what the wallet holds now", body = ConsumeResult),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn capture<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<ConsumeResult, SumidaError> {
  CreditService::new(state, session).capture(id).await
}

/// Drop a hold. The credits become spendable again and nothing is recorded as spent.
///
/// The body is not read.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = RELEASE,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_release",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The hold's identifier")),
  responses(
    (status = 200, description = "The hold as it now stands, and what is spendable again", body = ReservationResult),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn release<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<ReservationResult, SumidaError> {
  CreditService::new(state, session).release(id).await
}

/// Put a wallet's cached balance beside the sum of its ledger.
///
/// `diverged` true means the two disagree, which is a bug in this installation rather than a fact
/// about the customer. The answer is still `200` — the route reports the state, it does not fail on
/// it.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = AUDIT,
  context_path = MOUNT,
  tag = CREDIT_TAG,
  operation_id = "credit_audit",
  description = OWNERSHIP,
  params(("id" = i32, Path, description = "The wallet's identifier")),
  responses(
    (status = 200, description = "The cached balance, the ledger's sum, and whether they agree", body = WalletAudit),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn audit<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<WalletAudit, SumidaError> {
  CreditService::new(state, session).audit(id).await
}

impl<S: SumidaState> ApiController<S> for CreditController<S> {
  fn router(self, state: &S) -> Router<S> {
    // Owner / org-admin authorization is enforced inside the service.
    Router::new()
      .route(BALANCE, routing::get(balance::<S>))
      .route(TRANSACTIONS, routing::get(transactions::<S>))
      .route(GRANT, routing::post(grant::<S>))
      .route(CONSUME, routing::post(consume::<S>))
      .route(REFUND, routing::post(refund::<S>))
      .route(RESERVE, routing::post(reserve::<S>))
      .route(RESERVATIONS, routing::get(reservations::<S>))
      .route(CAPTURE, routing::post(capture::<S>))
      .route(RELEASE, routing::post(release::<S>))
      .route(AUDIT, routing::get(audit::<S>))
      .route_layer(user_guard!(state))
  }
}
