pub mod dto;

use chrono::Utc;
use entity::membership::{CreditTxKind, OwnerKind, PlanKind, PurchaseStatus, SubscriptionStatus};
use entity::prelude::*;
use error::SumidaError;
use infra::payment::{self, PaymentError, WebhookHeaders};
use infra::session::UserSession;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, IntoSimpleExpr, QueryFilter, TransactionTrait};

use self::dto::*;
use super::coupon::CouponService;
use super::credit::CreditService;
use super::subscription::SubscriptionService;
use super::{gen_code, resolve_owner};
use crate::SumidaState;
use crate::util::ap::{self, ApGuard};
use crate::util::org::organization_ancestors_subquery;

fn map_pay_err(e: PaymentError) -> SumidaError {
  match e {
    PaymentError::NotConfigured => SumidaError::PaymentNotConfigured,
    PaymentError::InvalidSignature => SumidaError::WebhookVerificationFailed,
    PaymentError::Unsupported => SumidaError::Unsupported("payment operation".into()),
    PaymentError::NotFound(id) => SumidaError::PaymentProviderError(format!("payment `{id}` not found")),
    PaymentError::Request(m) | PaymentError::Other(m) => SumidaError::PaymentProviderError(m),
  }
}

/// Merchant order id. For the mock provider, encode the amount so the deterministic
/// mock returns a matching PAID verification for the happy path.
fn order_code(provider: &str, amount: i64) -> String {
  match provider {
    "mock" => format!("mock:paid:{amount}:{}", gen_code("ord")),
    _ => gen_code("pur"),
  }
}

pub struct PaymentService<S: SumidaState> {
  state: S,
  session: Option<UserSession>,
}

impl<S: SumidaState> PaymentService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self {
      state,
      session: Some(session),
    }
  }

  /// System context (no session) — used by the webhook handler.
  pub fn system(state: S) -> Self {
    Self {
      state,
      session: None,
    }
  }

  fn session(&self) -> Result<&UserSession, SumidaError> {
    self.session.as_ref().ok_or(SumidaError::NoCredentials)
  }

  async fn authorize<C: ConnectionTrait>(
    &self,
    kind: &PlanKind,
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<(), SumidaError> {
    let session = self.session()?;
    if session.profile.is_admin {
      return Ok(());
    }
    match owner_kind {
      OwnerKind::Personal if session.profile.id == owner_id => Ok(()),
      OwnerKind::Personal => Err(SumidaError::NoAccessPermission),
      OwnerKind::Organization => {
        let ns = if matches!(kind, PlanKind::Credit) { "org:credit" } else { "org:subscription" };
        let start =
          Expr::col((entity::organizations::Entity, entity::organizations::Column::Id)).into_simple_expr();
        ap::ns(ns)
          .who(session)
          .action("manage")
          .object_in_subquery(organization_ancestors_subquery(start))
          .try_access(Organizations::find_by_id(owner_id), db)
          .await
      }
    }
  }

  // ---- public API ----

  pub async fn checkout(&self, req: CheckoutRequest) -> Result<CheckoutResult, SumidaError> {
    use entity::membership::membership_plans::Column as PlanCol;

    let session = self.session()?;
    let (owner_kind, owner_id) = resolve_owner(session, &req.owner_kind, req.owner_id);

    let txn = self.state.db().begin().await?;
    self.authorize(&req.kind, &owner_kind, owner_id, &txn).await?;

    let plan = MembershipPlans::find()
      .filter(PlanCol::Code.eq(req.plan_code.clone()))
      .filter(PlanCol::DeletedAt.is_null())
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::PlanNotFound(req.plan_code.clone()))?;

    if plan.kind != req.kind {
      return Err(SumidaError::BadRequest("plan kind mismatch".into()));
    }
    if !plan.is_enabled {
      return Err(SumidaError::BadRequest("plan is not enabled".into()));
    }
    if plan.scope != owner_kind {
      return Err(SumidaError::MembershipPlanScopeMismatch);
    }
    if plan.kind == PlanKind::Credit && plan.credit_amount.unwrap_or(0) <= 0 {
      return Err(SumidaError::BadRequest("credit plan must define a positive creditAmount".into()));
    }

    // Price breakdown: subtotal → coupon discount(s) → tax → total charged.
    let subtotal = plan.price;
    let mut codes: Vec<String> = Vec::new();
    if let Some(c) = &req.coupon_code {
      codes.push(c.clone());
    }
    if let Some(list) = &req.coupon_codes {
      codes.extend(list.iter().cloned());
    }
    codes.sort();
    codes.dedup();

    let mut discount = 0i64;
    let mut applied: Vec<String> = Vec::new();
    let mut redemption_ids: Vec<i32> = Vec::new();
    let mut all_stackable = true;
    for code in &codes {
      let a = CouponService::<S>::apply_in_txn(
        code,
        &plan.currency,
        subtotal,
        &owner_kind,
        owner_id,
        &plan.code,
        &txn,
      )
      .await?;
      discount += a.discount;
      all_stackable &= a.stackable;
      applied.push(a.code);
      redemption_ids.push(a.redemption_id);
    }
    if codes.len() > 1 && !all_stackable {
      return Err(SumidaError::BadRequest("one or more coupons are not stackable".into()));
    }
    let discount = discount.min(subtotal);
    let coupon_code = (!applied.is_empty()).then(|| applied.join(","));

    let taxable = (subtotal - discount).max(0);
    let tax_bp = settings::get().membership.tax_percent_bp.max(0) as i128;
    let tax = ((taxable as i128 * tax_bp) / 10_000) as i64;
    let total = taxable + tax;

    let provider = payment::provider();
    let code = order_code(provider.name(), total);

    MembershipPurchases::insert(MembershipPurchasesActiveModel {
      code: Set(code.clone()),
      kind: Set(plan.kind.clone()),
      plan_id: Set(plan.id),
      plan_code: Set(plan.code.clone()),
      owner_kind: Set(owner_kind),
      owner_id: Set(owner_id),
      status: Set(PurchaseStatus::Pending),
      amount: Set(total),
      subtotal: Set(subtotal),
      discount_amount: Set(discount),
      tax_amount: Set(tax),
      coupon_code: Set(coupon_code),
      currency: Set(plan.currency.clone()),
      provider: Set(provider.name().to_string()),
      ..Default::default()
    })
    .exec(&txn)
    .await?;

    // Link the coupon redemptions to this purchase so a failed/abandoned checkout
    // can be reversed.
    CouponService::<S>::link_redemptions(&redemption_ids, &code, &txn).await?;

    txn.commit().await?;

    Ok(CheckoutResult {
      purchase_code: code,
      amount: total,
      subtotal,
      discount_amount: discount,
      tax_amount: tax,
      currency: plan.currency,
      provider: provider.name().to_string(),
    })
  }

  pub async fn confirm(&self, req: ConfirmRequest) -> Result<ConfirmResult, SumidaError> {
    use entity::membership::membership_purchases::Column;

    let session = self.session()?;
    let triggered_by = session.profile.id;

    let txn = self.state.db().begin().await?;
    let purchase = MembershipPurchases::find()
      .filter(Column::Code.eq(req.purchase_code.clone()))
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::PurchaseNotFound(req.purchase_code.clone()))?;

    self.authorize(&purchase.kind, &purchase.owner_kind, purchase.owner_id, &txn).await?;

    // Commit either way: on a declined payment, settle has marked the purchase
    // failed and reversed its coupons — those writes must persist (a `?` here would
    // roll them back along with the error).
    let result = self.settle(&purchase, &req.payment_id, Some(triggered_by), &txn).await;
    txn.commit().await?;
    result
  }

  /// Refund a completed purchase: reverse the entitlement, cancel at the PG,
  /// and mark the purchase refunded.
  pub async fn refund(
    &self,
    purchase_code: &str,
    reason: Option<&str>,
  ) -> Result<PaymentRefundResult, SumidaError> {
    use entity::membership::membership_purchases::Column;

    let _session = self.session()?;
    let txn = self.state.db().begin().await?;
    let purchase = MembershipPurchases::find()
      .filter(Column::Code.eq(purchase_code.to_string()))
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::PurchaseNotFound(purchase_code.to_string()))?;
    self.authorize(&purchase.kind, &purchase.owner_kind, purchase.owner_id, &txn).await?;

    if purchase.status != PurchaseStatus::Completed {
      return Err(SumidaError::BadRequest("only completed purchases can be refunded".into()));
    }

    // Reverse the entitlement before cancelling at the PG.
    match purchase.kind {
      PlanKind::Subscription => {
        use entity::membership::subscriptions::Column as SubCol;
        let subs = Subscriptions::find()
          .filter(SubCol::OwnerKind.eq(purchase.owner_kind.clone()))
          .filter(SubCol::OwnerId.eq(purchase.owner_id))
          .filter(SubCol::PlanId.eq(purchase.plan_id))
          .filter(SubCol::Status.eq(SubscriptionStatus::Active))
          .all(&txn)
          .await?;
        for s in subs {
          Subscriptions::update(SubscriptionsActiveModel {
            id: Unchanged(s.id),
            status: Set(SubscriptionStatus::Canceled),
            canceled_at: Set(Some(Utc::now().fixed_offset())),
            ..Default::default()
          })
          .exec(&txn)
          .await?;
        }
      }
      PlanKind::Credit => {
        let plan = MembershipPlans::find_by_id(purchase.plan_id)
          .one(&txn)
          .await?
          .ok_or_else(|| SumidaError::PlanNotFound(purchase.plan_code.clone()))?;
        let units = plan.credit_amount.unwrap_or(0);
        if units > 0 {
          let idem = format!("refund:{}", purchase.code);
          CreditService::<S>::debit_in_txn(
            CreditTxKind::Refund,
            &purchase.owner_kind,
            purchase.owner_id,
            units,
            None,
            Some(&purchase.code),
            Some(&idem),
            None,
            &txn,
          )
          .await
          .map_err(|e| match e {
            SumidaError::InsufficientCredits { available, .. } => {
              SumidaError::RefundWouldExceedBalance(units, available)
            }
            other => other,
          })?;
        }
      }
    }

    let provider = payment::provider();
    let pay_ref = purchase.provider_payment_id.clone().unwrap_or_else(|| purchase.code.clone());
    let cancel = provider
      .cancel(&pay_ref, Some(purchase.amount), reason.unwrap_or("refund"))
      .await
      .map_err(map_pay_err)?;

    MembershipPurchases::update(MembershipPurchasesActiveModel {
      id: Unchanged(purchase.id),
      status: Set(PurchaseStatus::Refunded),
      refunded_at: Set(Some(Utc::now().fixed_offset())),
      ..Default::default()
    })
    .exec(&txn)
    .await?;

    txn.commit().await?;

    Ok(PaymentRefundResult {
      purchase_code: purchase.code,
      status: "refunded".to_string(),
      cancelled_amount: cancel.cancelled_amount,
    })
  }

  /// Process a PG webhook (signature-verified, idempotent). No session.
  pub async fn handle_webhook(&self, headers: WebhookHeaders, body: &[u8]) -> Result<(), SumidaError> {
    use entity::membership::membership_purchases::Column;
    use entity::membership::payment_webhook_events::Column as WhCol;

    let provider = payment::provider();
    let event = provider.verify_webhook(&headers, body).map_err(map_pay_err)?;

    let txn = self.state.db().begin().await?;

    // Idempotent: dedup by Standard-Webhooks id.
    if !headers.id.is_empty() {
      if PaymentWebhookEvents::find()
        .filter(WhCol::WebhookId.eq(headers.id.clone()))
        .one(&txn)
        .await?
        .is_some()
      {
        txn.commit().await?;
        return Ok(());
      }
      PaymentWebhookEvents::insert(PaymentWebhookEventsActiveModel {
        provider: Set(provider.name().to_string()),
        webhook_id: Set(headers.id.clone()),
        event_type: Set(event.event_type.clone()),
        payment_id: Set(event.payment_id.clone()),
        processed_at: Set(Some(Utc::now().fixed_offset())),
        ..Default::default()
      })
      .exec(&txn)
      .await?;
    }

    let purchase = MembershipPurchases::find()
      .filter(Column::Code.eq(event.payment_id.clone()))
      .one(&txn)
      .await?;

    if let Some(purchase) = purchase {
      if event.is_paid() && purchase.status == PurchaseStatus::Pending {
        self.settle(&purchase, &event.payment_id, None, &txn).await?;
      } else if event.is_cancelled() && purchase.status != PurchaseStatus::Refunded {
        MembershipPurchases::update(MembershipPurchasesActiveModel {
          id: Unchanged(purchase.id),
          status: Set(PurchaseStatus::Refunded),
          refunded_at: Set(Some(Utc::now().fixed_offset())),
          ..Default::default()
        })
        .exec(&txn)
        .await?;
      }
    }

    txn.commit().await?;
    Ok(())
  }

  /// Verify a payment, complete the purchase, and activate the entitlement.
  /// Idempotent: a completed purchase is returned unchanged.
  async fn settle<C: ConnectionTrait>(
    &self,
    purchase: &entity::membership::membership_purchases::Model,
    payment_id: &str,
    triggered_by: Option<i32>,
    txn: &C,
  ) -> Result<ConfirmResult, SumidaError> {
    if purchase.status == PurchaseStatus::Completed {
      return Ok(ConfirmResult {
        purchase_code: purchase.code.clone(),
        status: "completed".to_string(),
        kind: purchase.kind.clone(),
        subscription_id: None,
        wallet_id: None,
      });
    }

    let provider = payment::provider();
    let verification = provider.verify(payment_id).await.map_err(map_pay_err)?;

    let fail = |txn_status: PurchaseStatus| MembershipPurchasesActiveModel {
      id: Unchanged(purchase.id),
      status: Set(txn_status),
      ..Default::default()
    };

    if !verification.status.is_paid() {
      MembershipPurchases::update(fail(PurchaseStatus::Failed)).exec(txn).await?;
      CouponService::<S>::reverse_for_purchase(&purchase.code, txn).await?;
      return Err(SumidaError::PaymentNotCompleted(verification.raw_status));
    }
    if verification.amount != purchase.amount {
      MembershipPurchases::update(fail(PurchaseStatus::Failed)).exec(txn).await?;
      CouponService::<S>::reverse_for_purchase(&purchase.code, txn).await?;
      return Err(SumidaError::PaymentAmountMismatch {
        expected: purchase.amount,
        actual: verification.amount,
      });
    }

    MembershipPurchases::update(MembershipPurchasesActiveModel {
      id: Unchanged(purchase.id),
      status: Set(PurchaseStatus::Completed),
      provider_payment_id: Set(
        verification.transaction_id.clone().or_else(|| Some(payment_id.to_string())),
      ),
      paid_at: Set(Some(Utc::now().fixed_offset())),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    let plan = MembershipPlans::find_by_id(purchase.plan_id)
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::PlanNotFound(purchase.plan_code.clone()))?;

    let mut subscription_id = None;
    let mut wallet_id = None;

    match purchase.kind {
      PlanKind::Subscription => {
        use entity::membership::subscriptions::Column as SubCol;
        // Idempotency / dup guard: reuse an existing active subscription.
        let existing = Subscriptions::find()
          .filter(SubCol::OwnerKind.eq(purchase.owner_kind.clone()))
          .filter(SubCol::OwnerId.eq(purchase.owner_id))
          .filter(SubCol::PlanId.eq(plan.id))
          .filter(SubCol::Status.eq(SubscriptionStatus::Active))
          .one(txn)
          .await?;
        subscription_id = match existing {
          Some(s) => Some(s.id),
          None => Some(
            SubscriptionService::<S>::activate_in_txn(
              &plan,
              &purchase.owner_kind,
              purchase.owner_id,
              Some(&purchase.code),
              triggered_by,
              txn,
            )
            .await?
            .id,
          ),
        };
      }
      PlanKind::Credit => {
        let units = plan.credit_amount.unwrap_or(0);
        let result = CreditService::<S>::grant_in_txn(
          &purchase.owner_kind,
          purchase.owner_id,
          units,
          Some(&plan.code),
          None,
          Some(&purchase.code),
          // idempotency_key = purchase code → webhook/confirm retries never double-grant.
          Some(&purchase.code),
          txn,
        )
        .await?;
        wallet_id = Some(result.wallet_id);
      }
    }

    Ok(ConfirmResult {
      purchase_code: purchase.code.clone(),
      status: "completed".to_string(),
      kind: purchase.kind.clone(),
      subscription_id,
      wallet_id,
    })
  }
}
