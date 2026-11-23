pub mod dto;

use entity::membership::OwnerKind;
use entity::membership::membership_purchases::Column;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ActiveEnum,
  ColumnTrait,
  Condition,
  EntityTrait,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  QueryTrait,
  Select,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::{ConditionQuery, listed};
use crate::util::pagination::Pagination;

pub struct PurchaseService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> PurchaseService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Owner-scope: a platform administrator → all; otherwise own personal + own organization.
  ///
  /// Administration is asked of the rule (`platform:root#superuser`), never read off the session:
  /// a request on a secured API key carries its owner's `is_admin` but none of the owner's authority.
  async fn scope(
    &self,
    query: Select<MembershipPurchases>,
  ) -> Result<Select<MembershipPurchases>, SumidaError> {
    if Authz::is_superuser(&SubjectSet::of(&self.session), self.state.db()).await? {
      return Ok(query);
    }
    Ok(
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
      .await?
      .apply_if(listed(filter.owner_kind.as_ref()), |q, v| {
        q.filter(Column::OwnerKind.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(filter.owner_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::OwnerId)))
      .apply_if(listed(filter.kind.as_ref()), |q, v| {
        q.filter(Column::Kind.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(filter.status.as_ref()), |q, v| {
        q.filter(Column::Status.is_in(v.iter().map(|k| k.clone().into_value())))
      })
      .apply_if(listed(filter.plan_code.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::PlanCode)));

    let mut query = conditions.apply::<PurchaseFilter, _>(query)?;

    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by_schema::<PurchaseFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<PurchaseListItem>().all(db).await?;
    Ok((items, count))
  }
}
