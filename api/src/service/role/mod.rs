pub mod dto;

use core::{db, session::UserContext};
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  ActiveValue::*, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect,
  Select, TransactionTrait, sea_query::Expr,
};

use crate::{
  service::ApiService,
  util::{
    ap::{Ap, UserAp},
    crud::Pagination,
  },
};

use self::dto::*;

pub struct RoleService;

impl RoleService {
  pub async fn list(
    user_ctx: &UserContext,
    filter: RoleFilter,
    pagination: Pagination,
  ) -> Result<(Vec<RoleListItem>, u64), SumidaError> {
    use entity::roles::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(Self::select(user_ctx, Roles::find()));
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

  pub async fn get(user_ctx: &UserContext, id: i32) -> Result<Option<GetRole>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    Self::ap(user_ctx).action("read").check(Roles::find_by_id(id), &txn).await?;

    let Some(mut role) = Roles::find_by_id(id).into_model::<GetRole>().one(&txn).await? else {
      return Ok(None);
    };

    role.access_policies = UserAp::query().role(id).find_all(&txn).await?;
    let role = Some(role);
    txn.commit().await?;

    Ok(role)
  }

  pub async fn create(
    _user_ctx: &UserContext,
    data: CreateRole,
  ) -> Result<InsertResult<entity::roles::ActiveModel>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    let user_aps = data.access_policies.clone();
    let result = Roles::insert(data.into_active_model()).exec(&txn).await?;
    UserAp::query().role(result.last_insert_id).update(user_aps, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    user_ctx: &UserContext,
    id: i32,
    data: UpdateRole,
  ) -> Result<entity::roles::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;
    Self::ap(user_ctx).action("update").check(Roles::find_by_id(id), &txn).await?;

    if let Some(user_aps) = data.access_policies.clone() {
      UserAp::query().role(id).update(user_aps, &txn).await?;
    }

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Roles::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(user_ctx: &UserContext, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    Self::ap(user_ctx).action("delete").check(Roles::find_by_id(id), &txn).await?;

    UserAp::query().role(id).clear(&txn).await?;
    let result = Roles::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<Roles> for RoleService {
  fn select(user_ctx: &UserContext, query: Select<Roles>) -> Select<Roles> {
    Self::ap(user_ctx).action("read").apply(query)
  }

  fn ap(user_ctx: &UserContext) -> Ap {
    use entity::roles::Column;

    Ap::new("role") //
      .owner(user_ctx)
      .object(Expr::col((Roles, Column::Id)).cast_as("varchar"))
  }
}
