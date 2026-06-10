pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, TransactionTrait,
};
use sea_orm_crud::Optional;
use uuid::Uuid;

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

pub struct AccessPolicyService<S: SumidaState> {
  state: S,
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
    // Block privilege escalation where a non-admin self-grants a wildcard allow.
    ap::assert_policy_within_authority(
      self.session.profile.is_admin,
      &data.namespace,
      std::slice::from_ref(&data.object),
      &data.action,
      &data.user,
      &data.effect,
    )?;

    Ok(AccessPolicies::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    uuid: Uuid,
    data: UpdateAccessPolicy,
  ) -> Result<entity::access_policies::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", AccessPolicies::find_by_id(uuid), &txn).await?;

    // Since this is a partial update, validate against the 'final state' that merges the existing policy with the changes.
    // (Prevents stepwise circumvention, e.g. changing namespace to * while leaving effect alone.)
    if !self.session.profile.is_admin {
      let existing = AccessPolicies::find_by_id(uuid)
        .one(&txn)
        .await?
        .ok_or_else(|| SumidaError::NoData("AccessPolicy".to_owned(), uuid.to_string()))?;
      let Optional(namespace) = data.namespace.clone();
      let Optional(object) = data.object.clone();
      let Optional(action) = data.action.clone();
      let Optional(user) = data.user.clone();
      let Optional(effect) = data.effect.clone();
      ap::assert_policy_within_authority(
        false,
        &namespace.unwrap_or(existing.namespace),
        std::slice::from_ref(&object.unwrap_or(existing.object)),
        &action.unwrap_or(existing.action),
        &user.unwrap_or(existing.user),
        &effect.unwrap_or(existing.effect),
      )?;
    }

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
