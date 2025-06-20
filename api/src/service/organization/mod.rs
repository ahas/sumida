pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, Set,
  TransactionTrait,
};

use crate::util::crud::Pagination;

use self::dto::*;

pub struct OrganizationService;

impl OrganizationService {
  pub async fn list(
    filter: OrganizationFilter,
    pagination: Pagination,
  ) -> Result<(Vec<OrganizationListItem>, u64), SumidaError> {
    use entity::organizations::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let mut query = filter.apply(Organizations::find());
    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name,
      "createdAt" => Column::CreatedAt,
      "updatedAt" => Column::UpdatedAt,
      _ => Column::Id,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<OrganizationListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetOrganization>, SumidaError> {
    let db_conn = db::get_connection();
    Ok(Organizations::find_by_id(id).into_model::<GetOrganization>().one(db_conn).await?)
  }

  pub async fn create(
    data: CreateOrganization,
  ) -> Result<InsertResult<entity::organizations::ActiveModel>, SumidaError> {
    let db_conn = db::get_connection();
    Ok(Organizations::insert(data.into_active_model()).exec(db_conn).await?)
  }

  pub async fn update(
    id: i32,
    data: UpdateOrganization,
  ) -> Result<entity::organizations::Model, SumidaError> {
    let mut model = data.into_active_model();
    model.id = Set(id);

    let db_conn = db::get_connection();
    Ok(Organizations::update(model).exec(db_conn).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(Organizations::delete_by_id(id).exec(db_conn).await?)
  }
}
