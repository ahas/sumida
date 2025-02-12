pub mod dto;

use sea_orm::{ActiveValue::*, TransactionTrait};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, QueryTrait,
};
use sea_orm_crud::Optional;
use serde::Serialize;
use serde_json::{json, Map};

use crate::core::db;
use crate::core::error::AppError;
use crate::entity::prelude::*;
use crate::provider::RoleAbilityProvider;
use crate::util::crud::{search, Pagination};

use self::dto::*;

pub async fn list(
  filter: RoleFilter,
  pagination: Pagination,
) -> Result<(Vec<RoleListItem>, u64), AppError> {
  use crate::entity::roles::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let query = Roles::find()
    .apply_if(filter.name, |q, v| search(q, Column::Name, v))
    .apply_if(filter.description, |q, v| search(q, Column::Description, v));
  let count = query.clone().select_only().column(Column::Id).count(&txn).await?;
  let query = pagination.order_by(query, |col| match col {
    "name" => Column::Name,
    "createdAt" => Column::CreatedAt,
    "updatedAt" => Column::UpdatedAt,
    _ => Column::Id,
  });
  let query = pagination.paginate(query);
  let items = query.into_model::<RoleListItem>().all(&txn).await?;

  txn.commit().await?;

  Ok((items, count))
}

pub async fn get(id: i32) -> Result<Option<GetRole>, AppError> {
  let db_conn = db::get_connection();
  Ok(Roles::find_by_id(id).into_model::<GetRole>().one(db_conn).await?)
}

pub async fn create<T, R>(
  mut data: CreateRole,
) -> Result<InsertResult<crate::entity::roles::ActiveModel>, AppError>
where
  T: Default + Serialize,
  R: RoleAbilityProvider<T>,
{
  let Optional(ability) = data.ability;
  let default_role_ability = serde_json::to_value(R::get()).unwrap_or(json!({}));
  let ability =
    with_defailt_role_ability(default_role_ability.clone(), ability.unwrap_or(default_role_ability));

  data.ability = Optional(Some(ability));

  Ok(Roles::insert(data.into_active_model()).exec(db::get_connection()).await?)
}

pub async fn update<T, R>(id: i32, mut data: UpdateRole) -> Result<crate::entity::roles::Model, AppError>
where
  T: Default + Serialize,
  R: RoleAbilityProvider<T>,
{
  let Optional(ability) = data.ability;
  let default_role_ability = serde_json::to_value(R::get()).unwrap_or(json!({}));
  let ability =
    with_defailt_role_ability(default_role_ability.clone(), ability.unwrap_or(default_role_ability));

  data.ability = Optional(Some(ability));

  let mut model = data.into_active_model();
  model.id = Set(id);

  Ok(Roles::update(model).exec(db::get_connection()).await?)
}

pub async fn delete(id: i32) -> Result<DeleteResult, AppError> {
  let db_conn = db::get_connection();
  Ok(Roles::delete_by_id(id).exec(db_conn).await?)
}

fn with_defailt_role_ability(
  default_role_ability: serde_json::Value,
  role_ability: serde_json::Value,
) -> serde_json::Value {
  let role_ability = role_ability.as_object().unwrap();
  let mut result = default_role_ability.as_object().unwrap().clone();

  for (key, val) in role_ability {
    let default_ability = Map::new();
    let ability = val.as_object().unwrap_or(&default_ability);
    let ability = json!({
      "read": ability.get("read").unwrap_or(&json!(false)).as_bool().unwrap_or(false),
      "create": ability.get("create").unwrap_or(&json!(false)).as_bool().unwrap_or(false),
      "update": ability.get("update").unwrap_or(&json!(false)).as_bool().unwrap_or(false),
      "delete": ability.get("delete").unwrap_or(&json!(false)).as_bool().unwrap_or(false),
    });
    result.insert(key.clone(), ability);
  }

  serde_json::Value::Object(result)
}
