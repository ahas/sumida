pub mod dto;

use entity::membership::OwnerKind;
use entity::membership::membership_purchases::Column;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, QueryTrait, Select,
};

use self::dto::*;
use crate::SumidaState;
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

  pub async fn list(
    &self,
    filter: PurchaseFilter,
    pagination: Pagination,
  ) -> Result<(Vec<PurchaseListItem>, u64), SumidaError> {
    let db = self.state.db();
    let mut query = self
      .scope(MembershipPurchases::find())
      .apply_if(filter.owner_kind.clone(), |q, v| q.filter(Column::OwnerKind.eq(v)))
      .apply_if(filter.owner_id, |q, v| q.filter(Column::OwnerId.eq(v)))
      .apply_if(filter.kind.clone(), |q, v| q.filter(Column::Kind.eq(v)))
      .apply_if(filter.status.clone(), |q, v| q.filter(Column::Status.eq(v)))
      .apply_if(filter.plan_code.clone(), |q, v| q.filter(Column::PlanCode.eq(v)));

    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "createdAt" => Column::CreatedAt,
      _ => Column::Id,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<PurchaseListItem>().all(db).await?;
    Ok((items, count))
  }
}
