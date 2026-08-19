pub mod dto;

use entity::membership::OwnerKind;
use entity::membership::membership_purchases::Column;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ActiveEnum, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
  QueryTrait, Select,
};

use self::dto::*;
use crate::SumidaState;
use crate::util::filter::{ConditionQuery, listed};
use crate::util::pagination::Pagination;

pub struct PurchaseService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> PurchaseService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Owner-scope: admin → all; otherwise own personal + own organization.
  fn scope(&self, query: Select<MembershipPurchases>) -> Select<MembershipPurchases> {
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

  /// The owner scope first, the declared fields inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `PurchaseFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: PurchaseFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<PurchaseListItem>, u64), SumidaError> {
    let db = self.state.db();
    let query = self
      .scope(MembershipPurchases::find())
      .apply_if(listed(filter.owner_kind.as_ref()), |q, v| {
        q.filter(Column::OwnerKind.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(filter.owner_id.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(Column::OwnerId))
      })
      .apply_if(listed(filter.kind.as_ref()), |q, v| {
        q.filter(Column::Kind.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(filter.status.as_ref()), |q, v| {
        q.filter(Column::Status.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(filter.plan_code.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(Column::PlanCode))
      });

    let mut query = conditions.apply::<PurchaseFilter, _>(query)?;

    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by_schema::<PurchaseFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<PurchaseListItem>().all(db).await?;
    Ok((items, count))
  }
}
