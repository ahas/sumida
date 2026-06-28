pub mod dto;

use chrono::Utc;
use entity::membership::OwnerKind;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
  ColumnTrait, Condition, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
  QuerySelect, QueryTrait, Statement, TransactionTrait,
};

use self::dto::*;
use super::credit::CreditService;
use super::subscription::SubscriptionService;
use crate::SumidaState;
use crate::util::pagination::Pagination;

pub struct CouponService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> CouponService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  fn require_admin(&self) -> Result<(), SumidaError> {
    if self.session.profile.is_admin { Ok(()) } else { Err(SumidaError::OnlyAdminAllowed) }
  }

  pub async fn create(&self, data: CreateCoupon) -> Result<CouponCreated, SumidaError> {
    self.require_admin()?;
    // Exactly one reward: discount (percent_off), credits, or a free plan.
    let rewards = [data.percent_off.is_some(), data.grant_credits.is_some(), data.free_plan_code.is_some()];
    if rewards.iter().filter(|x| **x).count() != 1 {
      return Err(SumidaError::BadRequest(
        "coupon needs exactly one of percent_off / grant_credits / free_plan_code".into(),
      ));
    }

    let txn = self.state.db().begin().await?;
    let model = Coupons::insert(CouponsActiveModel {
      code: Set(data.code),
      percent_off: Set(data.percent_off),
      grant_credits: Set(data.grant_credits),
      free_plan_code: Set(data.free_plan_code),
      currency: Set(data.currency),
      max_redemptions: Set(data.max_redemptions),
      max_redemptions_per_owner: Set(data.max_redemptions_per_owner),
      requires_grant: Set(data.requires_grant),
      stackable: Set(data.stackable),
      expires_at: Set(data.expires_at),
      ..Default::default()
    })
    .exec_with_returning(&txn)
    .await?;

    if let Some(plan_codes) = data.plan_codes {
      for plan_code in plan_codes {
        CouponPlans::insert(CouponPlansActiveModel {
          coupon_id: Set(model.id),
          plan_code: Set(plan_code),
          ..Default::default()
        })
        .exec(&txn)
        .await?;
      }
    }
    txn.commit().await?;
    Ok(CouponCreated {
      id: model.id,
      code: model.code,
    })
  }

  /// Issue a coupon to many owners at once (campaign distribution). Each owner
  /// follows the single-grant lifecycle; the summary counts the outcomes.
  pub async fn grant_bulk(
    &self,
    coupon_id: i32,
    data: BulkGrantCoupon,
  ) -> Result<BulkGrantResult, SumidaError> {
    self.require_admin()?;
    if Coupons::find_by_id(coupon_id).one(self.state.db()).await?.is_none() {
      return Err(SumidaError::NoData("Coupon".into(), coupon_id.to_string()));
    }
    let mut result = BulkGrantResult {
      granted: 0,
      skipped: 0,
      conflicts: 0,
    };
    for owner in &data.owners {
      match self.grant_once(coupon_id, owner).await {
        Ok(true) => result.granted += 1,
        Ok(false) => result.skipped += 1,
        Err(SumidaError::Conflict(_)) => result.conflicts += 1,
        Err(e) => return Err(e),
      }
    }
    Ok(result)
  }

  pub async fn list(
    &self,
    filter: CouponFilter,
    pagination: Pagination,
  ) -> Result<(Vec<CouponItem>, u64), SumidaError> {
    self.require_admin()?;
    let db = self.state.db();
    let mut query =
      Coupons::find().apply_if(filter.is_active, |q, v| q.filter(CouponsColumn::IsActive.eq(v)));
    let count = query.clone().select_only().column(CouponsColumn::Id).count(db).await?;
    query = pagination.order_by(query, |col| match col {
      "code" => CouponsColumn::Code,
      _ => CouponsColumn::Id,
    });
    query = pagination.paginate(query);
    let items = query.into_model::<CouponItem>().all(db).await?;
    Ok((items, count))
  }

  pub async fn update(&self, id: i32, data: UpdateCoupon) -> Result<CouponUpdated, SumidaError> {
    self.require_admin()?;
    let mut am = CouponsActiveModel {
      id: Unchanged(id),
      ..Default::default()
    };
    if let Some(v) = data.is_active {
      am.is_active = Set(v);
    }
    if let Some(v) = data.max_redemptions {
      am.max_redemptions = Set(Some(v));
    }
    if let Some(v) = data.max_redemptions_per_owner {
      am.max_redemptions_per_owner = Set(Some(v));
    }
    if let Some(v) = data.requires_grant {
      am.requires_grant = Set(v);
    }
    if let Some(v) = data.stackable {
      am.stackable = Set(v);
    }
    if let Some(v) = data.expires_at {
      am.expires_at = Set(Some(v));
    }
    Coupons::update(am)
      .exec(self.state.db())
      .await
      .map_err(|_| SumidaError::NoData("Coupon".into(), id.to_string()))?;
    Ok(CouponUpdated)
  }

  pub async fn delete(&self, id: i32) -> Result<CouponDeleted, SumidaError> {
    self.require_admin()?;
    Coupons::delete_by_id(id).exec(self.state.db()).await?;
    Ok(CouponDeleted)
  }

  async fn find_grant<C: ConnectionTrait>(
    coupon_id: i32,
    owner_kind: &OwnerKind,
    owner_id: i32,
    db: &C,
  ) -> Result<Option<entity::membership::coupon_grants::Model>, SumidaError> {
    Ok(
      CouponGrants::find()
        .filter(CouponGrantsColumn::CouponId.eq(coupon_id))
        .filter(CouponGrantsColumn::OwnerKind.eq(owner_kind.clone()))
        .filter(CouponGrantsColumn::OwnerId.eq(owner_id))
        .one(db)
        .await?,
    )
  }

  /// Issue a grant (lifecycle-aware). Returns whether it was newly (re)issued or a
  /// no-op; an already-redeemed grant is a `Conflict`.
  async fn grant_once(&self, coupon_id: i32, owner: &GrantCoupon) -> Result<bool, SumidaError> {
    let db = self.state.db();
    match Self::find_grant(coupon_id, &owner.owner_kind, owner.owner_id, db).await? {
      None => {
        CouponGrants::insert(CouponGrantsActiveModel {
          coupon_id: Set(coupon_id),
          owner_kind: Set(owner.owner_kind.clone()),
          owner_id: Set(owner.owner_id),
          ..Default::default()
        })
        .exec(db)
        .await?;
        Ok(true)
      }
      Some(g) if g.revoked_at.is_some() => {
        // Re-issue: the prior recall makes a fresh grant deliberate.
        CouponGrants::update(CouponGrantsActiveModel {
          id: Unchanged(g.id),
          redeemed_at: Set(None),
          revoked_at: Set(None),
          ..Default::default()
        })
        .exec(db)
        .await?;
        Ok(true)
      }
      Some(g) if g.redeemed_at.is_some() => Err(SumidaError::Conflict(
        "coupon already redeemed by this owner; revoke it to re-issue".into(),
      )),
      Some(_) => Ok(false), // issued & unused → idempotent no-op
    }
  }

  /// Issue a coupon to a specific owner ("distribute").
  /// - no grant → issue;
  /// - already issued (unused) → idempotent no-op (safe for campaign retries);
  /// - revoked → re-issue (the recall enabled it);
  /// - already redeemed → **409** (revoke first to re-issue — no silent re-grant).
  pub async fn grant_to(&self, coupon_id: i32, data: GrantCoupon) -> Result<CouponGranted, SumidaError> {
    self.require_admin()?;
    if Coupons::find_by_id(coupon_id).one(self.state.db()).await?.is_none() {
      return Err(SumidaError::NoData("Coupon".into(), coupon_id.to_string()));
    }
    self.grant_once(coupon_id, &data).await?;
    Ok(CouponGranted)
  }

  /// Recall a coupon from an owner. A revoked grant can no longer be redeemed;
  /// re-granting re-issues it. Idempotent.
  pub async fn revoke(&self, coupon_id: i32, data: GrantCoupon) -> Result<CouponRevoked, SumidaError> {
    self.require_admin()?;
    let db = self.state.db();
    let Some(g) = Self::find_grant(coupon_id, &data.owner_kind, data.owner_id, db).await? else {
      return Err(SumidaError::NoData("CouponGrant".into(), coupon_id.to_string()));
    };
    if g.revoked_at.is_none() {
      CouponGrants::update(CouponGrantsActiveModel {
        id: Unchanged(g.id),
        revoked_at: Set(Some(Utc::now().into())),
        ..Default::default()
      })
      .exec(db)
      .await?;
    }
    Ok(CouponRevoked)
  }

  /// List a coupon's grants (admin) — for issued/redeemed/revoked analytics.
  pub async fn grants(
    &self,
    coupon_id: i32,
    pagination: Pagination,
  ) -> Result<(Vec<CouponGrantItem>, u64), SumidaError> {
    self.require_admin()?;
    let db = self.state.db();
    let mut query = CouponGrants::find().filter(CouponGrantsColumn::CouponId.eq(coupon_id));
    let count = query.clone().select_only().column(CouponGrantsColumn::Id).count(db).await?;
    query = pagination.order_by(query, |_| CouponGrantsColumn::Id);
    query = pagination.paginate(query);
    let items = query.into_model::<CouponGrantItem>().all(db).await?;
    Ok((items, count))
  }

  /// Coupons issued to the caller (their personal self or their organization) that
  /// are still active and unexpired.
  pub async fn my_coupons(&self, pagination: Pagination) -> Result<(Vec<MyCouponItem>, u64), SumidaError> {
    let db = self.state.db();
    let grants = CouponGrants::find()
      .filter(CouponGrantsColumn::RedeemedAt.is_null())
      .filter(CouponGrantsColumn::RevokedAt.is_null())
      .filter(
        Condition::any()
          .add(
            Condition::all()
              .add(CouponGrantsColumn::OwnerKind.eq(OwnerKind::Personal))
              .add(CouponGrantsColumn::OwnerId.eq(self.session.profile.id)),
          )
          .add(
            Condition::all()
              .add(CouponGrantsColumn::OwnerKind.eq(OwnerKind::Organization))
              .add(CouponGrantsColumn::OwnerId.eq(self.session.profile.organization.id)),
          ),
      )
      .all(db)
      .await?;
    let mut ids: Vec<i32> = grants.into_iter().map(|g| g.coupon_id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
      return Ok((vec![], 0));
    }

    let now: DateTimeWithTimeZone = Utc::now().into();
    let mut query = Coupons::find()
      .filter(CouponsColumn::Id.is_in(ids))
      .filter(CouponsColumn::IsActive.eq(true))
      .filter(
        Condition::any()
          .add(CouponsColumn::ExpiresAt.is_null())
          .add(CouponsColumn::ExpiresAt.gt(now)),
      );
    let count = query.clone().select_only().column(CouponsColumn::Id).count(db).await?;
    query = pagination.order_by(query, |col| match col {
      "code" => CouponsColumn::Code,
      _ => CouponsColumn::Id,
    });
    query = pagination.paginate(query);
    let items = query.into_model::<MyCouponItem>().all(db).await?;
    Ok((items, count))
  }

  /// Shared eligibility + atomic consumption for one coupon use: grant gate
  /// (consume the grant), per-owner cap, global cap, and the redemption-ledger row.
  /// Returns the redemption id. Failures are `BadRequest`.
  async fn consume_coupon<C: ConnectionTrait>(
    coupon: &entity::membership::coupons::Model,
    owner_kind: &OwnerKind,
    owner_id: i32,
    txn: &C,
  ) -> Result<i32, SumidaError> {
    let mut grant_id: Option<i32> = None;
    if coupon.requires_grant {
      let grant = Self::find_grant(coupon.id, owner_kind, owner_id, txn)
        .await?
        .ok_or_else(|| SumidaError::BadRequest("coupon is not available to this owner".into()))?;
      if grant.revoked_at.is_some() {
        return Err(SumidaError::BadRequest("coupon grant was revoked".into()));
      }
      if grant.redeemed_at.is_some() {
        return Err(SumidaError::BadRequest("coupon already redeemed by this owner".into()));
      }
      CouponGrants::update(CouponGrantsActiveModel {
        id: Unchanged(grant.id),
        redeemed_at: Set(Some(Utc::now().into())),
        ..Default::default()
      })
      .exec(txn)
      .await?;
      grant_id = Some(grant.id);
    } else if let Some(per_owner) = coupon.max_redemptions_per_owner {
      let used = CouponRedemptions::find()
        .filter(CouponRedemptionsColumn::CouponId.eq(coupon.id))
        .filter(CouponRedemptionsColumn::OwnerKind.eq(owner_kind.clone()))
        .filter(CouponRedemptionsColumn::OwnerId.eq(owner_id))
        .count(txn)
        .await?;
      if used >= per_owner as u64 {
        return Err(SumidaError::BadRequest("coupon redemption limit reached for this owner".into()));
      }
    }

    #[derive(FromQueryResult)]
    struct Redeemed {
      redeemed_count: i32,
    }
    let updated = Redeemed::find_by_statement(Statement::from_sql_and_values(
      txn.get_database_backend(),
      r#"UPDATE coupons SET redeemed_count = redeemed_count + 1
         WHERE id = $1 AND (max_redemptions IS NULL OR redeemed_count < max_redemptions)
         RETURNING redeemed_count"#,
      [coupon.id.into()],
    ))
    .one(txn)
    .await?;
    if updated.is_none() {
      return Err(SumidaError::BadRequest("coupon redemption limit reached".into()));
    }

    let redemption = CouponRedemptions::insert(CouponRedemptionsActiveModel {
      coupon_id: Set(coupon.id),
      owner_kind: Set(owner_kind.clone()),
      owner_id: Set(owner_id),
      grant_id: Set(grant_id),
      ..Default::default()
    })
    .exec_with_returning(txn)
    .await?;
    Ok(redemption.id)
  }

  /// Apply a **discount** coupon (percent_off) at checkout: validate active /
  /// expiry / currency-scope / plan-scope, then consume it. Reward coupons
  /// (credits / free-plan) are rejected here — those are redeemed via `redeem`.
  pub(crate) async fn apply_in_txn<C: ConnectionTrait>(
    code: &str,
    currency: &str,
    subtotal: i64,
    owner_kind: &OwnerKind,
    owner_id: i32,
    plan_code: &str,
    txn: &C,
  ) -> Result<AppliedCoupon, SumidaError> {
    let coupon = Coupons::find()
      .filter(CouponsColumn::Code.eq(code))
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::BadRequest("coupon not found".into()))?;

    if !coupon.is_active {
      return Err(SumidaError::BadRequest("coupon is not active".into()));
    }
    if let Some(exp) = coupon.expires_at {
      if exp <= Utc::now() {
        return Err(SumidaError::BadRequest("coupon has expired".into()));
      }
    }
    let Some(pct) = coupon.percent_off else {
      return Err(SumidaError::BadRequest("this is a reward coupon — redeem it instead".into()));
    };
    // Currency-scope: a regional promo applies only to its currency.
    if let Some(scope) = &coupon.currency {
      if scope != currency {
        return Err(SumidaError::BadRequest("coupon is not valid for this currency".into()));
      }
    }
    // Plan-scope: if the coupon lists plans, the checkout plan must be among them.
    let plan_rows = CouponPlans::find().filter(CouponPlansColumn::CouponId.eq(coupon.id)).all(txn).await?;
    if !plan_rows.is_empty() && !plan_rows.iter().any(|p| p.plan_code == plan_code) {
      return Err(SumidaError::BadRequest("coupon is not valid for this plan".into()));
    }

    let discount = ((subtotal as i128 * pct.clamp(0, 100) as i128 / 100) as i64).clamp(0, subtotal);
    let redemption_id = Self::consume_coupon(&coupon, owner_kind, owner_id, txn).await?;
    Ok(AppliedCoupon {
      code: coupon.code,
      discount,
      stackable: coupon.stackable,
      redemption_id,
    })
  }

  /// Redeem a **reward** coupon (credits / free plan) for the calling user.
  pub async fn redeem(&self, data: RedeemCoupon) -> Result<RedeemResult, SumidaError> {
    let owner_kind = OwnerKind::Personal;
    let owner_id = self.session.profile.id;

    let txn = self.state.db().begin().await?;
    let coupon = Coupons::find()
      .filter(CouponsColumn::Code.eq(data.code.clone()))
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::BadRequest("coupon not found".into()))?;

    if !coupon.is_active {
      return Err(SumidaError::BadRequest("coupon is not active".into()));
    }
    if let Some(exp) = coupon.expires_at {
      if exp <= Utc::now() {
        return Err(SumidaError::BadRequest("coupon has expired".into()));
      }
    }
    if coupon.percent_off.is_some() {
      return Err(SumidaError::BadRequest("this is a discount coupon — use it at checkout".into()));
    }

    Self::consume_coupon(&coupon, &owner_kind, owner_id, &txn).await?;

    let mut result = RedeemResult {
      reward: String::new(),
      credits_granted: None,
      subscription_id: None,
    };
    if let Some(credits) = coupon.grant_credits {
      let expiry_at = settings::get()
        .membership
        .credit_default_expiry_days
        .map(|d| (Utc::now() + chrono::Duration::days(d as i64)).into());
      let reference = format!("coupon:{}", coupon.code);
      CreditService::<S>::grant_in_txn(
        &owner_kind,
        owner_id,
        credits,
        Some(&coupon.code),
        expiry_at,
        Some(&reference),
        Some(&reference),
        &txn,
      )
      .await?;
      result.reward = "credits".into();
      result.credits_granted = Some(credits);
    } else if let Some(plan_code) = &coupon.free_plan_code {
      let plan = MembershipPlans::find()
        .filter(entity::membership::membership_plans::Column::Code.eq(plan_code.clone()))
        .filter(entity::membership::membership_plans::Column::DeletedAt.is_null())
        .one(&txn)
        .await?
        .ok_or_else(|| SumidaError::PlanNotFound(plan_code.clone()))?;
      if plan.kind != entity::membership::PlanKind::Subscription {
        return Err(SumidaError::BadRequest("free_plan_code is not a subscription plan".into()));
      }
      // Don't double-activate.
      let existing = Subscriptions::find()
        .filter(entity::membership::subscriptions::Column::OwnerKind.eq(owner_kind.clone()))
        .filter(entity::membership::subscriptions::Column::OwnerId.eq(owner_id))
        .filter(entity::membership::subscriptions::Column::PlanId.eq(plan.id))
        .filter(
          entity::membership::subscriptions::Column::Status
            .eq(entity::membership::SubscriptionStatus::Active),
        )
        .one(&txn)
        .await?;
      if existing.is_some() {
        return Err(SumidaError::DuplicateActiveSubscription);
      }
      let sub = SubscriptionService::<S>::activate_in_txn(
        &plan,
        &owner_kind,
        owner_id,
        Some(&coupon.code),
        Some(owner_id),
        &txn,
      )
      .await?;
      result.reward = "free-plan".into();
      result.subscription_id = Some(sub.id);
    }

    txn.commit().await?;
    Ok(result)
  }

  /// Link the just-applied redemptions to their purchase.
  pub(crate) async fn link_redemptions<C: ConnectionTrait>(
    redemption_ids: &[i32],
    purchase_code: &str,
    txn: &C,
  ) -> Result<(), SumidaError> {
    for id in redemption_ids {
      CouponRedemptions::update(CouponRedemptionsActiveModel {
        id: Unchanged(*id),
        purchase_code: Set(Some(purchase_code.to_string())),
        ..Default::default()
      })
      .exec(txn)
      .await?;
    }
    Ok(())
  }

  /// Reverse all coupon redemptions of a purchase (failed / abandoned checkout):
  /// give back the grant, decrement the global count, and delete the redemption.
  pub(crate) async fn reverse_for_purchase<C: ConnectionTrait>(
    purchase_code: &str,
    txn: &C,
  ) -> Result<(), SumidaError> {
    let rows = CouponRedemptions::find()
      .filter(CouponRedemptionsColumn::PurchaseCode.eq(purchase_code))
      .all(txn)
      .await?;
    for r in rows {
      if let Some(grant_id) = r.grant_id {
        CouponGrants::update(CouponGrantsActiveModel {
          id: Unchanged(grant_id),
          redeemed_at: Set(None),
          ..Default::default()
        })
        .exec(txn)
        .await?;
      }
      txn
        .execute(Statement::from_sql_and_values(
          txn.get_database_backend(),
          "UPDATE coupons SET redeemed_count = GREATEST(redeemed_count - 1, 0) WHERE id = $1",
          [r.coupon_id.into()],
        ))
        .await?;
      CouponRedemptions::delete_by_id(r.id).exec(txn).await?;
    }
    Ok(())
  }
}
