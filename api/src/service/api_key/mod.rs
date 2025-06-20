pub mod dto;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use core::db;
use entity::prelude::*;
use error::SumidaError;
use rand::{TryRngCore, rngs::OsRng};
use sea_orm::{
  ActiveValue::Unchanged, DbErr, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait,
  QuerySelect, Set, SqlErr, TransactionTrait,
};

use crate::util::crud::Pagination;

use self::dto::*;

pub struct ApiKeyService;

impl ApiKeyService {
  pub async fn list(
    filter: ApiKeyFilter,
    pagination: Pagination,
  ) -> Result<(Vec<ApiKeyListItem>, u64), SumidaError> {
    use entity::api_keys::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let mut query = filter.apply(ApiKeys::find());
    let count = query.clone().select_only().column(Column::Key).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "key" => Column::Key,
      "user" => Column::User,
      _ => Column::Key,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<ApiKeyListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(key: &str) -> Result<Option<GetApiKey>, SumidaError> {
    let db_conn = db::get_connection();
    Ok(ApiKeys::find_by_id(key).into_model::<GetApiKey>().one(db_conn).await?)
  }

  fn generate_token_bytes(entropy_bytes: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; entropy_bytes];
    OsRng.try_fill_bytes(&mut buf)?;

    Ok(URL_SAFE_NO_PAD.encode(buf))
  }

  fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
  }

  pub async fn create(
    data: CreateApiKey,
  ) -> Result<InsertResult<entity::api_keys::ActiveModel>, SumidaError> {
    let db_conn = db::get_connection();

    loop {
      let key = match Self::generate_token_bytes(64) {
        Ok(v) => v,
        _ => {
          continue;
        }
      };

      let mut active_model = data.clone().into_active_model();
      active_model.key = Set(key);

      match ApiKeys::insert(active_model).exec(db_conn).await {
        Ok(result) => return Ok(result),
        Err(err) if Self::is_unique_violation(&err) => {
          continue;
        }
        Err(err) => return Err(err.into()),
      }
    }
  }

  pub async fn update(key: &str, data: UpdateApiKey) -> Result<entity::api_keys::Model, SumidaError> {
    let mut model = data.into_active_model();
    model.key = Unchanged(key.to_string());

    let db_conn = db::get_connection();
    Ok(ApiKeys::update(model).exec(db_conn).await?)
  }

  pub async fn delete(key: &str) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(ApiKeys::delete_by_id(key).exec(db_conn).await?)
  }
}
