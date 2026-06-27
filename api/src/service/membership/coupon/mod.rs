pub mod dto;

use chrono::Utc;
use entity::membership::OwnerKind;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
  ColumnTrait, Condition, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
  QuerySelect, QueryTrait, Statement,
};

use self::dto::*;
use super::owner_kind_str;
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
    if data.percent_off.is_none() && data.amount_off.is_none() {
      return Err(SumidaError::BadRequest("coupon needs percent_off or amount_off".into()));
    }
    if data.amount_off.is_some() && data.currency.is_none() {
      return Err(SumidaError::BadRequest("amount_off coupons need a currency".into()));
    }
    let model = Coupons::insert(CouponsActiveModel {
      code: Set(data.code),
      percent_off: Set(data.percent_off),
      amount_off: Set(data.amount_off),
      currency: Set(data.currency),
      max_redemptions: Set(data.max_redemptions),
      max_redemptions_per_owner: Set(data.max_redemptions_per_owner),
      requires_grant: Set(data.requires_grant),
      expires_at: Set(data.expires_at),
      ..Default::default()
    })
    .exec_with_returning(self.state.db())
    .await?;
    Ok(CouponCreated { id: model.id, code: model.code })
  }

  pub async fn list(
    &self,
    filter: CouponFilter,
    pagination: Pagination,
  ) -> Result<(Vec<CouponItem>, u64), SumidaError> {
    self.require_admin()?;
    let db = self.state.db();
    let mut query = Coupons::find()
      .apply_if(filter.is_active, |q, v| q.filter(CouponsColumn::IsActive.eq(v)));
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
    let mut am = CouponsActiveModel { id: Unchanged(id), ..Default::default() };
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

  /// Issue a coupon to a specific owner ("distribute"). Idempotent per owner. With
  /// the coupon's `requires_grant` set this also makes the coupon redeemable by
  /// that owner; either way the owner sees it in their "my coupons" list.
  pub async fn grant_to(&self, coupon_id: i32, data: GrantCoupon) -> Result<CouponGranted, SumidaError> {
    self.require_admin()?;
    let db = self.state.db();
    if Coupons::find_by_id(coupon_id).one(db).await?.is_none() {
      return Err(SumidaError::NoData("Coupon".into(), coupon_id.to_string()));
    }
    db.execute(Statement::from_sql_and_values(
      db.get_database_backend(),
      r#"INSERT INTO coupon_grants (coupon_id, owner_kind, owner_id) VALUES ($1, $2, $3)
         ON CONFLICT (coupon_id, owner_kind, owner_id) DO NOTHING"#,
      [coupon_id.into(), owner_kind_str(&data.owner_kind).into(), data.owner_id.into()],
    ))
    .await?;
    Ok(CouponGranted)
  }

  /// Coupons issued to the caller (their personal self or their organization) that
  /// are still active and unexpired.
  pub async fn my_coupons(
    &self,
    pagination: Pagination,
  ) -> Result<(Vec<MyCouponItem>, u64), SumidaError> {
    let db = self.state.db();
    let grants = CouponGrants::find()
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
      .filter(Condition::any().add(CouponsColumn::ExpiresAt.is_null()).add(CouponsColumn::ExpiresAt.gt(now)));
    let count = query.clone().select_only().column(CouponsColumn::Id).count(db).await?;
    query = pagination.order_by(query, |col| match col {
      "code" => CouponsColumn::Code,
      _ => CouponsColumn::Id,
    });
    query = pagination.paginate(query);
    let items = query.into_model::<MyCouponItem>().all(db).await?;
    Ok((items, count))
  }

  /// Validate a coupon and atomically redeem one use, returning the discount (in
  /// minor units, clamped to `subtotal`) and the coupon code. Runs in the caller's
  /// checkout transaction. All failures are `BadRequest`.
  pub(crate) async fn apply_in_txn<C: ConnectionTrait>(
    code: &str,
    currency: &str,
    subtotal: i64,
    owner_kind: &OwnerKind,
    owner_id: i32,
    txn: &C,
  ) -> Result<(i64, String), SumidaError> {
    let coupon = Coupons::find()
      .filter(CouponsColumn::Code.eq(code))
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::BadRequest("coupon not found".into()))?;

    if !coupon.is_active {
      return Err(SumidaError::BadRequest("coupon is not active".into()));
    }
    if let Some(exp) = coupon.expires_at {
      let now: DateTimeWithTimeZone = Utc::now().into();
      if exp <= now {
        return Err(SumidaError::BadRequest("coupon has expired".into()));
      }
    }

    // Grant-gated coupons are redeemable only by owners they were issued to.
    if coupon.requires_grant {
      let granted = CouponGrants::find()
        .filter(CouponGrantsColumn::CouponId.eq(coupon.id))
        .filter(CouponGrantsColumn::OwnerKind.eq(owner_kind.clone()))
        .filter(CouponGrantsColumn::OwnerId.eq(owner_id))
        .one(txn)
        .await?;
      if granted.is_none() {
        return Err(SumidaError::BadRequest("coupon is not available to this owner".into()));
      }
    }

    // Per-owner cap: count prior non-failed purchases bearing this coupon code.
    if let Some(per_owner) = coupon.max_redemptions_per_owner {
      #[derive(FromQueryResult)]
      struct CountRow {
        total: i64,
      }
      let used = CountRow::find_by_statement(Statement::from_sql_and_values(
        txn.get_database_backend(),
        r#"SELECT COUNT(*)::bigint AS total FROM membership_purchases
           WHERE coupon_code = $1 AND owner_kind = $2 AND owner_id = $3 AND status <> 'failed'"#,
        [code.into(), owner_kind_str(owner_kind).into(), owner_id.into()],
      ))
      .one(txn)
      .await?
      .map(|r| r.total)
      .unwrap_or(0);
      if used >= per_owner as i64 {
        return Err(SumidaError::BadRequest("coupon redemption limit reached for this owner".into()));
      }
    }

    let discount = if let Some(pct) = coupon.percent_off {
      (subtotal as i128 * pct.clamp(0, 100) as i128 / 100) as i64
    } else if let Some(amount) = coupon.amount_off {
      if coupon.currency.as_deref() != Some(currency) {
        return Err(SumidaError::BadRequest("coupon currency mismatch".into()));
      }
      amount
    } else {
      0
    };
    let discount = discount.clamp(0, subtotal);

    // Atomic redeem under the cap (serialized on the row by ON CONFLICT-style guard).
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

    Ok((discount, coupon.code))
  }
}
