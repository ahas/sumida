pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::{
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  PaginatorTrait,
  QuerySelect,
  TransactionTrait,
};
use uuid::Uuid;

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

pub struct AccessPolicyService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> AccessPolicyService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: AccessPolicyFilter,
    pagination: Pagination,
  ) -> Result<(Vec<AccessPolicyListItem>, u64), SumidaError> {
    use entity::access_policies::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(AccessPolicies::find()));
    let count = query.clone().select_only().column(Column::Uuid).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "namespace" => Column::Namespace,
      "effect" => Column::Effect,
      "object" => Column::Object,
      "user" => Column::User,
      "createdAt" => Column::CreatedAt,
      _ => Column::CreatedAt,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<AccessPolicyListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, uuid: Uuid) -> Result<Option<GetAccessPolicy>, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("read", AccessPolicies::find_by_id(uuid), &txn).await?;

    let access_policy = AccessPolicies::find_by_id(uuid).into_model::<GetAccessPolicy>().one(&txn).await?;
    txn.commit().await?;

    Ok(access_policy)
  }

  pub async fn create(
    &self,
    data: CreateAccessPolicy,
  ) -> Result<InsertResult<entity::access_policies::ActiveModel>, SumidaError> {
    Ok(AccessPolicies::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    uuid: Uuid,
    data: UpdateAccessPolicy,
  ) -> Result<entity::access_policies::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", AccessPolicies::find_by_id(uuid), &txn).await?;

    let mut model = data.into_active_model();
    model.uuid = Set(uuid);

    let result = AccessPolicies::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, uuid: Uuid) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", AccessPolicies::find_by_id(uuid), &txn).await?;

    let result = AccessPolicies::delete_by_id(uuid).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, AccessPolicies> for AccessPolicyService<S> {
  fn ap(&self) -> ApCondition {
    use entity::access_policies::Column;

    ap::ns("access_policy").who(&self.session).join(AccessPolicies, Column::Uuid)
  }

  fn select(&self, query: sea_orm::Select<AccessPolicies>) -> sea_orm::Select<AccessPolicies> {
    self.ap().action("read").apply(query)
  }
}
