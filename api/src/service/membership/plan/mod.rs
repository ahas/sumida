pub mod dto;

use chrono::Utc;
use entity::membership::membership_plans::Column;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  DeleteResult,
  EntityTrait,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace membership_plan service = membership_plan`).
const RESOURCE: &str = "membership_plan";

pub struct PlanService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> PlanService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 요금제는 전역 설정이다 — `membership_plan#view` 는
  /// `service:membership_plan->operate_read` 하나로만 열린다.
  fn select(&self, query: Select<MembershipPlans>) -> Result<Select<MembershipPlans>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), query)?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `PlanFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: PlanFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<PlanListItem>, u64), SumidaError> {
    let db = self.state.db();
    let mut query =
      conditions.apply::<PlanFilter, _>(filter.apply(self.select(MembershipPlans::find())?))?;
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by_schema::<PlanFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<PlanListItem>().all(db).await?;
    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetPlan>, SumidaError> {
    let db = self.state.db();
    let Some(plan) = self
      .select(MembershipPlans::find_by_id(id))?
      .into_model::<PlanListItem>()
      .one(db)
      .await?
    else {
      return Ok(None);
    };

    let actions = PlanActions::find()
      .filter(PlanActionsColumn::PlanId.eq(id))
      .into_model::<PlanActionItem>()
      .all(db)
      .await?;

    Ok(Some(GetPlan { plan, actions }))
  }

  async fn sync_actions<C: ConnectionTrait>(
    plan_id: i32,
    actions: Vec<PlanActionInput>,
    db: &C,
  ) -> Result<(), SumidaError> {
    PlanActions::delete_many()
      .filter(PlanActionsColumn::PlanId.eq(plan_id))
      .exec(db)
      .await?;
    for action in actions {
      PlanActions::insert(PlanActionsActiveModel {
        plan_id: Set(plan_id),
        action: Set(action.action),
        credit_cost: Set(action.credit_cost),
        quota_per_period: Set(action.quota_per_period),
        ..Default::default()
      })
      .exec(db)
      .await?;
    }
    Ok(())
  }

  pub async fn create(
    &self,
    data: CreatePlan,
  ) -> Result<entity::membership::membership_plans::Model, SumidaError> {
    let txn = self.state.db().begin().await?;

    let currency = data.currency.unwrap_or_else(|| settings::get().membership.default_currency.clone());
    let actions = data.actions;

    let model = MembershipPlans::insert(MembershipPlansActiveModel {
      code: Set(data.code),
      name: Set(data.name),
      description: Set(data.description),
      kind: Set(data.kind),
      scope: Set(data.scope),
      price: Set(data.price),
      currency: Set(currency),
      billing_interval: Set(data.billing_interval),
      duration_days: Set(data.duration_days),
      trial_days: Set(data.trial_days),
      credit_amount: Set(data.credit_amount),
      is_enabled: Set(data.is_enabled),
      ..Default::default()
    })
    .exec_with_returning(&txn)
    .await?;

    Self::sync_actions(model.id, actions, &txn).await?;
    txn.commit().await?;

    Ok(model)
  }

  pub async fn update(&self, id: i32, data: UpdatePlan) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

    let mut am = MembershipPlansActiveModel {
      id: Unchanged(id),
      ..Default::default()
    };
    if let Some(v) = data.name {
      am.name = Set(v);
    }
    if let Some(v) = data.description {
      am.description = Set(Some(v));
    }
    if let Some(v) = data.price {
      am.price = Set(v);
    }
    if let Some(v) = data.currency {
      am.currency = Set(v);
    }
    if let Some(v) = data.billing_interval {
      am.billing_interval = Set(Some(v));
    }
    if let Some(v) = data.duration_days {
      am.duration_days = Set(Some(v));
    }
    if let Some(v) = data.trial_days {
      am.trial_days = Set(Some(v));
    }
    if let Some(v) = data.credit_amount {
      am.credit_amount = Set(Some(v));
    }
    if let Some(v) = data.is_enabled {
      am.is_enabled = Set(v);
    }
    MembershipPlans::update(am).exec(&txn).await?;

    if let Some(actions) = data.actions {
      Self::sync_actions(id, actions, &txn).await?;
    }

    txn.commit().await?;
    Ok(())
  }

  /// Soft delete: set `deleted_at`. The plan is retired but preserved for
  /// subscriptions/purchases that reference it (FK is ON DELETE RESTRICT).
  pub async fn delete(&self, id: i32) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;

    MembershipPlans::update(MembershipPlansActiveModel {
      id: Unchanged(id),
      deleted_at: Set(Some(Utc::now())),
      is_enabled: Set(false),
      ..Default::default()
    })
    .exec(&txn)
    .await?;

    txn.commit().await?;
    Ok(())
  }

  /// Hard delete, used only by admin tooling/tests when no references exist.
  pub async fn purge(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;
    let result = MembershipPlans::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}
