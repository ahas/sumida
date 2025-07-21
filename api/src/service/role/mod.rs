pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  ActiveValue::*, ColumnTrait, Condition, DeleteResult, EntityTrait, InsertResult, IntoActiveModel,
  PaginatorTrait, QueryFilter, QuerySelect, TransactionTrait,
};

use crate::util::crud::Pagination;

use self::dto::*;

pub struct RoleService;

impl RoleService {
  pub async fn find_access_policies(
    role_id: Option<i32>,
    user_id: Option<i32>,
  ) -> Result<Vec<RoleAccessPolicy>, SumidaError> {
    use entity::access_policies::{Column as ApColumn, Entity as ApEntity};

    let db = db::get_connection();
    let mut cond = Condition::any();

    if let Some(role_id) = role_id {
      cond = cond.add(ApColumn::User.eq(format!("role:{role_id}")))
    }

    if let Some(user_id) = user_id {
      cond = cond.add(ApColumn::User.eq(format!("user:{user_id}")))
    }

    let result = ApEntity::find().filter(cond).into_model::<RoleAccessPolicy>().all(db).await?;

    Ok(result)
  }

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
    let db_conn = db::get_connection();
    let Some(mut role) = Roles::find_by_id(id).into_model::<GetRole>().one(db_conn).await? else {
      return Ok(None);
    };
    role.access_policies = Self::find_access_policies(Some(id), None).await?;

    Ok(Some(role))
  }

  pub async fn create(data: CreateRole) -> Result<InsertResult<entity::roles::ActiveModel>, SumidaError> {
    Ok(Roles::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(id: i32, data: UpdateRole) -> Result<entity::roles::Model, SumidaError> {
    let mut model = data.into_active_model();
    model.id = Set(id);

    Ok(Roles::update(model).exec(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(Roles::delete_by_id(id).exec(db_conn).await?)
  }
}
