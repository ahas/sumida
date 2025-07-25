pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  ActiveValue::*, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect,
  TransactionTrait,
};

use crate::util::{ap::UserAp, crud::Pagination};

use self::dto::*;

pub struct RoleService;

impl RoleService {
  pub async fn list(
    filter: RoleFilter,
    pagination: Pagination,
  ) -> Result<(Vec<RoleListItem>, u64), SumidaError> {
    use entity::roles::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let mut query = filter.apply(Roles::find());
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

  pub async fn get(id: i32) -> Result<Option<GetRole>, SumidaError> {
    let db = db::get_connection();
    let Some(mut role) = Roles::find_by_id(id).into_model::<GetRole>().one(db).await? else {
      return Ok(None);
    };

    role.access_policies = UserAp::query().role(id).find_all(db).await?;

    Ok(Some(role))
  }

  pub async fn create(data: CreateRole) -> Result<InsertResult<entity::roles::ActiveModel>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    let user_aps = data.access_policies.clone();
    let result = Roles::insert(data.into_active_model()).exec(&txn).await?;
    UserAp::query().role(result.last_insert_id).update(user_aps, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(id: i32, data: UpdateRole) -> Result<entity::roles::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;

    if let Some(user_aps) = data.access_policies.clone() {
      UserAp::query().role(id).update(user_aps, &txn).await?;
    }

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Roles::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    UserAp::query().role(id).clear(&txn).await?;
    let result = Roles::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}
