pub mod dto;

use crate::core::db;
use crate::core::error::AppError;
use crate::entity::prelude::*;
use crate::util::crud::{search, Pagination};

use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QueryFilter,
  QuerySelect, QueryTrait, Set, TransactionTrait,
};

use self::dto::*;

pub async fn list(
  filter: OrganizationFilter,
  pagination: Pagination,
) -> Result<(Vec<OrganizationListItem>, u64), AppError> {
  use crate::entity::organizations::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let query = Organizations::find()
    .apply_if(filter.name, |q, v| search(q, Column::Name, v))
    .apply_if(filter.parent, |q, v| q.filter(Column::Parent.eq(v)));
  let count = query.clone().select_only().column(Column::Id).count(&txn).await?;
  let query = pagination.order_by(query, |col| match col {
    "name" => Column::Name,
    "createdAt" => Column::CreatedAt,
    "updatedAt" => Column::UpdatedAt,
    _ => Column::Id,
  });
  let query = pagination.paginate(query);
  let items = query.into_model::<OrganizationListItem>().all(&txn).await?;

  txn.commit().await?;

  Ok((items, count))
}

pub async fn get(id: i32) -> Result<Option<GetOrganization>, AppError> {
  let db_conn = db::get_connection();
  Ok(Organizations::find_by_id(id).into_model::<GetOrganization>().one(db_conn).await?)
}

pub async fn create(
  data: CreateOrganization,
) -> Result<InsertResult<crate::entity::organizations::ActiveModel>, AppError> {
  let db_conn = db::get_connection();
  Ok(Organizations::insert(data.into_active_model()).exec(db_conn).await?)
}

pub async fn update(
  id: i32,
  data: UpdateOrganization,
) -> Result<crate::entity::organizations::Model, AppError> {
  let mut model = data.into_active_model();
  model.id = Set(id);

  let db_conn = db::get_connection();
  Ok(Organizations::update(model).exec(db_conn).await?)
}

pub async fn delete(id: i32) -> Result<DeleteResult, AppError> {
  let db_conn = db::get_connection();
  Ok(Organizations::delete_by_id(id).exec(db_conn).await?)
}
