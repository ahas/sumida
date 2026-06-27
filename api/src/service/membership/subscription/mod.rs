pub mod dto;

use chrono::{Duration, Utc};
use entity::membership::subscriptions::Column;
use entity::membership::{OwnerKind, PlanKind, PurchaseStatus, SubscriptionEventKind, SubscriptionStatus};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::sea_query::Expr;
use sea_orm::{
  ColumnTrait, Condition, ConnectionTrait, EntityTrait, IntoSimpleExpr, PaginatorTrait, QueryFilter,
  QuerySelect, QueryTrait, Select, TransactionTrait,
};
use serde_json::json;

use self::dto::*;
use super::{gen_code, notify, resolve_owner};
use crate::SumidaState;
use crate::util::ap::{self, ApGuard};
use crate::util::org::organization_ancestors_subquery;
use crate::util::pagination::Pagination;

fn status_str(status: &SubscriptionStatus) -> &'static str {
  match status {
    SubscriptionStatus::Pending => "pending",
    SubscriptionStatus::Active => "active",
    SubscriptionStatus::PastDue => "past-due",
    SubscriptionStatus::Canceled => "canceled",
    SubscriptionStatus::Expired => "expired",
  }
}

pub struct SubscriptionService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> SubscriptionService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  // ---- authorization & scoping ----

  /// Restrict a list/query to subscriptions the caller may see:
  /// admin → all; otherwise own personal + own organization.
  fn scope(&self, query: Select<Subscriptions>) -> Select<Subscriptions> {
    if self.session.profile.is_admin {
      return query;
    }
    query.filter(
      Condition::any()
        .add(
          Condition::all()
            .add(Column::OwnerKind.eq(OwnerKind::Personal))
            .add(Column::OwnerId.eq(self.session.profile.id)),
        )
        .add(
          Condition::all()
            .add(Column::OwnerKind.eq(OwnerKind::Organization))
            .add(Column::OwnerId.eq(self.session.profile.organization.id)),
        ),
    )
  }

  async fn authorize<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    db: &C,
  ) -> Result<(), SumidaError> {
    if self.session.profile.is_admin {
      return Ok(());
    }
    match owner_kind {
      OwnerKind::Personal if self.session.profile.id == owner_id => Ok(()),
      OwnerKind::Personal => Err(SumidaError::NoAccessPermission),
      OwnerKind::Organization if action == "read" && self.session.profile.organization.id == owner_id => {
        Ok(())
      }
      OwnerKind::Organization => {
        // Mirror UserService::update org-scoping: evaluate the org-ancestors subquery
        // against the target organization row (the `"ap"` alias only exists in the
        // lateral-join path, so this must go through `try_access`, not `check_allowed`).
        let start = Expr::col((
          entity::organizations::Entity,
          entity::organizations::Column::Id,
        ))
        .into_simple_expr();
        ap::ns("org:subscription")
          .who(&self.session)
          .action(action)
          .object_in_subquery(organization_ancestors_subquery(start))
          .try_access(Organizations::find_by_id(owner_id), db)
          .await
      }
    }
  }

  async fn record_event<C: ConnectionTrait>(
    &self,
    subscription_id: i32,
    kind: SubscriptionEventKind,
    old_status: Option<&str>,
    new_status: Option<&str>,
    reason: Option<&str>,
    txn: &C,
  ) -> Result<(), SumidaError> {
    SubscriptionEvents::insert(SubscriptionEventsActiveModel {
      subscription_id: Set(subscription_id),
      event_kind: Set(kind),
      old_status: Set(old_status.map(|s| s.to_string())),
      new_status: Set(new_status.map(|s| s.to_string())),
      reason: Set(reason.map(|s| s.to_string())),
      triggered_by_user_id: Set(Some(self.session.profile.id)),
      ..Default::default()
    })
    .exec(txn)
    .await?;
    Ok(())
  }

  /// Snapshot a plan's actions as a JSON object `{action: quota_per_period|null}`.
  /// Key existence = coverage (checked via `jsonb_exists`); the value carries the
  /// per-period quota (`null` = unlimited). Snapshotted at activation so plan
  /// edits don't retroactively change a live subscription's coverage.
  pub(crate) async fn snapshot_allowed_actions<C: ConnectionTrait>(
    plan_id: i32,
    txn: &C,
  ) -> Result<serde_json::Value, SumidaError> {
    let rows = PlanActions::find().filter(PlanActionsColumn::PlanId.eq(plan_id)).all(txn).await?;
    let mut map = serde_json::Map::new();
    for a in rows {
      map.insert(a.action, a.quota_per_period.map(|q| json!(q)).unwrap_or(serde_json::Value::Null));
    }
    Ok(serde_json::Value::Object(map))
  }

  /// Build an active subscription from a plan (snapshot + period + created event),
  /// WITHOUT creating a purchase or checking session authorization. Used by the
  /// payment flow once a purchase is verified (webhooks have no session).
  pub(crate) async fn activate_in_txn<C: ConnectionTrait>(
    plan: &entity::membership::membership_plans::Model,
    owner_kind: &OwnerKind,
    owner_id: i32,
    reference_code: Option<&str>,
    triggered_by: Option<i32>,
    txn: &C,
  ) -> Result<entity::membership::subscriptions::Model, SumidaError> {
    let allowed = Self::snapshot_allowed_actions(plan.id, txn).await?;

    let now = Utc::now();
    let duration = plan.duration_days.unwrap_or(30).max(0);
    let period_end = now + Duration::days(duration as i64);

    let sub = Subscriptions::insert(SubscriptionsActiveModel {
      code: Set(gen_code("sub")),
      plan_id: Set(plan.id),
      plan_code: Set(plan.code.clone()),
      owner_kind: Set(owner_kind.clone()),
      owner_id: Set(owner_id),
      status: Set(SubscriptionStatus::Active),
      current_period_start: Set(now.into()),
      current_period_end: Set(period_end.into()),
      allowed_actions: Set(allowed),
      ..Default::default()
    })
    .exec_with_returning(txn)
    .await?;

    SubscriptionEvents::insert(SubscriptionEventsActiveModel {
      subscription_id: Set(sub.id),
      event_kind: Set(SubscriptionEventKind::Created),
      new_status: Set(Some("active".to_string())),
      reference_code: Set(reference_code.map(|s| s.to_string())),
      triggered_by_user_id: Set(triggered_by),
      ..Default::default()
    })
    .exec(txn)
    .await?;

    Ok(sub)
  }

  // ---- public API ----

  pub async fn list(
    &self,
    filter: SubscriptionFilter,
    pagination: Pagination,
  ) -> Result<(Vec<SubscriptionListItem>, u64), SumidaError> {
    let db = self.state.db();
    let mut query = self
      .scope(Subscriptions::find())
      .apply_if(filter.owner_kind.clone(), |q, v| q.filter(Column::OwnerKind.eq(v)))
      .apply_if(filter.owner_id, |q, v| q.filter(Column::OwnerId.eq(v)))
      .apply_if(filter.status.clone(), |q, v| q.filter(Column::Status.eq(v)))
      .apply_if(filter.plan_code.clone(), |q, v| q.filter(Column::PlanCode.eq(v)));

    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "createdAt" => Column::CreatedAt,
      "currentPeriodEnd" => Column::CurrentPeriodEnd,
      _ => Column::Id,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<SubscriptionListItem>().all(db).await?;
    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetSubscription>, SumidaError> {
    let db = self.state.db();
    let Some(sub) = Subscriptions::find_by_id(id).into_model::<GetSubscription>().one(db).await? else {
      return Ok(None);
    };
    self.authorize(&sub.owner_kind, sub.owner_id, "read", db).await?;
    Ok(Some(sub))
  }

  pub async fn subscribe(&self, data: CreateSubscription) -> Result<SubscriptionCreated, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &data.owner_kind, data.owner_id);
    let txn = self.state.db().begin().await?;
    self.authorize(&owner_kind, owner_id, "manage", &txn).await?;

    // Idempotency: same key already created a subscription → return it.
    if let Some(key) = &data.idempotency_key {
      if let Some(existing) = Subscriptions::find()
        .filter(Column::IdempotencyKey.eq(key.clone()))
        .one(&txn)
        .await?
      {
        txn.commit().await?;
        return Ok(SubscriptionCreated { id: existing.id, code: existing.code });
      }
    }

    let plan = MembershipPlans::find()
      .filter(entity::membership::membership_plans::Column::Code.eq(data.plan_code.clone()))
      .filter(entity::membership::membership_plans::Column::DeletedAt.is_null())
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::PlanNotFound(data.plan_code.clone()))?;

    if plan.kind != PlanKind::Subscription {
      return Err(SumidaError::BadRequest("plan is not a subscription plan".into()));
    }
    if !plan.is_enabled {
      return Err(SumidaError::BadRequest("plan is not enabled".into()));
    }
    if plan.scope != owner_kind {
      return Err(SumidaError::MembershipPlanScopeMismatch);
    }
    if data.auto_renew && data.billing_key.is_none() {
      return Err(SumidaError::BadRequest("auto_renew requires a billing_key".into()));
    }

    // At most one active subscription per (owner, plan).
    let existing_active = Subscriptions::find()
      .filter(Column::OwnerKind.eq(owner_kind.clone()))
      .filter(Column::OwnerId.eq(owner_id))
      .filter(Column::PlanId.eq(plan.id))
      .filter(Column::Status.eq(SubscriptionStatus::Active))
      .one(&txn)
      .await?;
    if existing_active.is_some() {
      return Err(SumidaError::DuplicateActiveSubscription);
    }

    let allowed = Self::snapshot_allowed_actions(plan.id, &txn).await?;

    let now = Utc::now();
    let duration = plan.duration_days.unwrap_or(30).max(0);
    let period_end = now + Duration::days(duration as i64);
    let code = gen_code("sub");

    let sub = Subscriptions::insert(SubscriptionsActiveModel {
      code: Set(code.clone()),
      plan_id: Set(plan.id),
      plan_code: Set(plan.code.clone()),
      owner_kind: Set(owner_kind.clone()),
      owner_id: Set(owner_id),
      status: Set(SubscriptionStatus::Active),
      current_period_start: Set(now.into()),
      current_period_end: Set(period_end.into()),
      allowed_actions: Set(allowed),
      idempotency_key: Set(data.idempotency_key.clone()),
      auto_renew: Set(data.auto_renew),
      billing_key: Set(data.billing_key.clone()),
      ..Default::default()
    })
    .exec_with_returning(&txn)
    .await?;

    // Canonical purchase record.
    MembershipPurchases::insert(MembershipPurchasesActiveModel {
      code: Set(gen_code("pur")),
      kind: Set(PlanKind::Subscription),
      plan_id: Set(plan.id),
      plan_code: Set(plan.code.clone()),
      owner_kind: Set(owner_kind.clone()),
      owner_id: Set(owner_id),
      status: Set(PurchaseStatus::Completed),
      amount: Set(plan.price),
      currency: Set(plan.currency.clone()),
      paid_at: Set(Some(now.into())),
      ..Default::default()
    })
    .exec(&txn)
    .await?;

    self
      .record_event(sub.id, SubscriptionEventKind::Created, None, Some("active"), Some(&code), &txn)
      .await?;

    txn.commit().await?;

    // Best-effort receipt (personal owners with an email only).
    let email = match owner_kind {
      OwnerKind::Personal => {
        Users::find_by_id(owner_id).one(self.state.db()).await.ok().flatten().and_then(|u| u.email)
      }
      OwnerKind::Organization => None,
    };
    notify::<S, _, _>(
      "subscription-created-receipt",
      email.as_deref(),
      json!({ "plan_code": plan.code, "current_period_end": period_end.to_rfc3339() }),
      self.state.db(),
    )
    .await;

    Ok(SubscriptionCreated { id: sub.id, code })
  }

  pub async fn cancel(&self, id: i32, at_period_end: bool) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(sub) = Subscriptions::find_by_id(id).one(&txn).await? else {
      return Err(SumidaError::NoData("Subscription".into(), id.to_string()));
    };
    self.authorize(&sub.owner_kind, sub.owner_id, "manage", &txn).await?;
    let old = status_str(&sub.status);

    if at_period_end {
      Subscriptions::update(SubscriptionsActiveModel {
        id: Unchanged(id),
        cancel_at_period_end: Set(true),
        ..Default::default()
      })
      .exec(&txn)
      .await?;
      self
        .record_event(id, SubscriptionEventKind::Canceled, Some(old), Some(old), Some("cancel-at-period-end"), &txn)
        .await?;
    } else {
      Subscriptions::update(SubscriptionsActiveModel {
        id: Unchanged(id),
        status: Set(SubscriptionStatus::Canceled),
        canceled_at: Set(Some(Utc::now().into())),
        ..Default::default()
      })
      .exec(&txn)
      .await?;
      self
        .record_event(id, SubscriptionEventKind::Canceled, Some(old), Some("canceled"), None, &txn)
        .await?;
    }

    txn.commit().await?;
    Ok(())
  }

  pub async fn renew(&self, id: i32) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(sub) = Subscriptions::find_by_id(id).one(&txn).await? else {
      return Err(SumidaError::NoData("Subscription".into(), id.to_string()));
    };
    self.authorize(&sub.owner_kind, sub.owner_id, "manage", &txn).await?;

    let plan = MembershipPlans::find_by_id(sub.plan_id).one(&txn).await?;
    let duration = plan.and_then(|p| p.duration_days).unwrap_or(30).max(0);

    let now_dt: sea_orm::prelude::DateTimeWithTimeZone = Utc::now().into();
    let base = if sub.current_period_end > now_dt { sub.current_period_end } else { now_dt };
    let new_end = base + Duration::days(duration as i64);
    let old = status_str(&sub.status);

    // A renewal begins a new period: advancing current_period_start resets any
    // per-period action quota (usage is keyed by period_start).
    Subscriptions::update(SubscriptionsActiveModel {
      id: Unchanged(id),
      status: Set(SubscriptionStatus::Active),
      current_period_start: Set(base),
      current_period_end: Set(new_end),
      cancel_at_period_end: Set(false),
      // New period → allow the "expiring soon" notice to fire again.
      expiring_notified_at: Set(None),
      ..Default::default()
    })
    .exec(&txn)
    .await?;
    self
      .record_event(id, SubscriptionEventKind::Renewed, Some(old), Some("active"), None, &txn)
      .await?;

    txn.commit().await?;
    Ok(())
  }

  /// Enable or disable auto-renewal. Enabling requires a billing key — supplied
  /// here or already stored on the subscription. Disabling keeps the stored key.
  pub async fn set_auto_renew(&self, id: i32, data: SetAutoRenew) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(sub) = Subscriptions::find_by_id(id).one(&txn).await? else {
      return Err(SumidaError::NoData("Subscription".into(), id.to_string()));
    };
    self.authorize(&sub.owner_kind, sub.owner_id, "manage", &txn).await?;

    if data.enabled && data.billing_key.is_none() && sub.billing_key.is_none() {
      return Err(SumidaError::BadRequest("enabling auto_renew requires a billing_key".into()));
    }

    let mut update = SubscriptionsActiveModel {
      id: Unchanged(id),
      auto_renew: Set(data.enabled),
      ..Default::default()
    };
    if let Some(key) = data.billing_key {
      update.billing_key = Set(Some(key));
    }
    Subscriptions::update(update).exec(&txn).await?;

    txn.commit().await?;
    Ok(())
  }
}
