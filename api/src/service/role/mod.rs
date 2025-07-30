pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::{db, session::UserSession};
use sea_orm::{
  ActiveValue::*, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect,
  Select, TransactionTrait, sea_query::Expr,
};

use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard, ApUser};
use crate::util::crud::Pagination;

use self::dto::*;

pub struct RoleService {
  session: UserSession,
}

impl RoleService {
  pub fn new(session: UserSession) -> Self {
    Self { session }
  }

  pub async fn list(
    &self,
    filter: RoleFilter,
    pagination: Pagination,
  ) -> Result<(Vec<RoleListItem>, u64), SumidaError> {
    use entity::roles::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(self.select(Roles::find()));
    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name,
      "createdAt" => Column::CreatedAt,
      "updatedAt" => Column::UpdatedAt,
      _ => Column::Id,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<RoleListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetRole>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().object(id).action("read").try_access(&txn).await?;

    let Some(mut role) = Roles::find_by_id(id).into_model::<GetRole>().one(&txn).await? else {
      return Ok(None);
    };

    role.access_policies = ap::for_users().user(ApUser::Role(id)).find_all(&txn).await?;
    let role = Some(role);
    txn.commit().await?;

    Ok(role)
  }

  pub async fn create(
    &self,
    data: CreateRole,
  ) -> Result<InsertResult<entity::roles::ActiveModel>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    let user_aps = data.access_policies.clone();
    let result = Roles::insert(data.into_active_model()).exec(&txn).await?;
    ap::for_users().user(ApUser::Role(result.last_insert_id)).update(user_aps, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(&self, id: i32, data: UpdateRole) -> Result<entity::roles::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().object(id).action("update").try_access(&txn).await?;

    if let Some(user_aps) = data.access_policies.clone() {
      ap::for_users().user(ApUser::Role(id)).update(user_aps, &txn).await?;
    }

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Roles::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().object(id).action("delete").try_access(&txn).await?;

    ap::for_users().user(ApUser::Role(id)).clear(&txn).await?;
    let result = Roles::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<ApCondition, Roles> for RoleService {
  fn select(&self, query: Select<Roles>) -> Select<Roles> {
    self.ap().action("read").apply(query)
  }

  fn ap(&self) -> ApCondition {
    use entity::roles::Column;

    ap::ns("role") //
      .who(&self.session)
      .object_ref(Expr::col((Roles, Column::Id)).cast_as("varchar"))
  }
}
