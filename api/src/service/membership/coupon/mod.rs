pub mod dto;

use chrono::Utc;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{
  ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QuerySelect,
  QueryTrait, Statement,
};

use self::dto::*;
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

  /// Validate a coupon and atomically redeem one use, returning the discount (in
  /// minor units, clamped to `subtotal`) and the coupon code. Runs in the caller's
  /// checkout transaction. All failures are `BadRequest`.
  pub(crate) async fn apply_in_txn<C: ConnectionTrait>(
    code: &str,
    currency: &str,
    subtotal: i64,
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
