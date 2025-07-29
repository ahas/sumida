pub mod dto;

use infra::{db, session::UserContext};
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, Select, Set,
  TransactionTrait, sea_query::Expr,
};

use crate::service::ApiService;
use crate::util::ap::{Ap, ApGuard};
use crate::util::crud::Pagination;

use self::dto::*;

pub struct OrganizationService {
  user_ctx: UserContext,
}

impl OrganizationService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self { user_ctx }
  }

  pub async fn list(
    &self,
    filter: OrganizationFilter,
    pagination: Pagination,
  ) -> Result<(Vec<OrganizationListItem>, u64), SumidaError> {
    use entity::organizations::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(self.select(Organizations::find()));
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

  pub async fn get(&self, id: i32) -> Result<Option<GetOrganization>, SumidaError> {
    Ok(
      self
        .select(Organizations::find_by_id(id))
        .into_model::<GetOrganization>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateOrganization,
  ) -> Result<InsertResult<entity::organizations::ActiveModel>, SumidaError> {
    Ok(Organizations::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateOrganization,
  ) -> Result<entity::organizations::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("update").check(Organizations::find_by_id(id), &txn).await?;

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Organizations::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("delete").check(Organizations::find_by_id(id), &txn).await?;

    let result = Organizations::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<Ap, Organizations> for OrganizationService {
  fn select(&self, query: Select<Organizations>) -> Select<Organizations> {
    self.ap().action("read").apply(query)
  }

  fn ap(&self) -> Ap {
    use entity::organizations::Column;

    Ap::new("organization") //
      .owner(&self.user_ctx)
      .object(Expr::col((Organizations, Column::Id)).cast_as("varchar"))
  }
}
