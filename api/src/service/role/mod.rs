pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{ActiveValue::*, TransactionTrait};
use sea_orm::{DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect};
use sea_orm_crud::Optional;
use serde_json::json;

use crate::util::crud::Pagination;

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
    let db_conn = db::get_connection();
    Ok(Roles::find_by_id(id).into_model::<GetRole>().one(db_conn).await?)
  }

  pub async fn create(
    mut data: CreateRole,
  ) -> Result<InsertResult<entity::roles::ActiveModel>, SumidaError> {
    let Optional(ability) = data.ability;
    let default_role_ability = serde_json::to_value(R::get()).unwrap_or(json!({}));
    let ability = Self::with_defailt_role_ability(
      default_role_ability.clone(),
      ability.unwrap_or(default_role_ability),
    );

    data.ability = Optional(Some(ability));

    Ok(Roles::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(id: i32, mut data: UpdateRole) -> Result<entity::roles::Model, SumidaError> {
    let Optional(ability) = data.ability;
    let default_role_ability = serde_json::to_value(R::get()).unwrap_or(json!({}));
    let ability = Self::with_defailt_role_ability(
      default_role_ability.clone(),
      ability.unwrap_or(default_role_ability),
    );

    data.ability = Optional(Some(ability));

    let mut model = data.into_active_model();
    model.id = Set(id);

    Ok(Roles::update(model).exec(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(Roles::delete_by_id(id).exec(db_conn).await?)
  }

  fn with_defailt_role_ability(
    default_role_ability: serde_json::Value,
    role_ability: serde_json::Value,
  ) -> serde_json::Value {
    let default_role_ability = default_role_ability.as_object().cloned().unwrap_or_default();
    let mut role_ability = role_ability.as_object().cloned().unwrap_or_default();

    for (key, val) in default_role_ability {
      if role_ability.contains_key(&key) {
        continue;
      }

      role_ability.insert(key.to_string(), val.clone());
    }

    serde_json::Value::Object(role_ability)
  }
}
