pub mod dto;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use entity::prelude::*;
use error::SumidaError;
use infra::{db, session::UserSession};
use rand::{TryRngCore, rngs::OsRng};
use sea_orm::{
  ActiveValue::Unchanged,
  DbErr, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, JoinType,
  PaginatorTrait, QuerySelect, RelationTrait, Select, Set, SqlErr, TransactionTrait,
  prelude::Expr,
  sea_query::{Alias, Asterisk},
};

use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::crud::Pagination;
use crate::{service::ApiService, util::ap::ApUser};

use self::dto::*;

pub struct ApiKeyService {
  session: UserSession,
}

impl ApiKeyService {
  pub fn new(session: UserSession) -> Self {
    ApiKeyService { session }
  }

  pub async fn list(
    &self,
    filter: ApiKeyFilter,
    pagination: Pagination,
  ) -> Result<(Vec<ApiKeyListItem>, u64), SumidaError> {
    use entity::api_keys::Column;

    let txn = db::get_connection().begin().await?;
    let col_username = Expr::col("user_username");
    let col_full_name = Expr::col("user_full_name");

    let mut query = filter.apply(self.select(ApiKeys::find()));
    let count = query.clone().select_only().column(Column::Key).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "key" => Column::Key.into_simple_expr(),
      "username" => col_username.into_simple_expr(),
      "name" => col_full_name.into_simple_expr(),
      _ => Column::Key.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<ApiKeyListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, key: &str) -> Result<Option<GetApiKey>, SumidaError> {
    let txn = db::get_connection().begin().await?;

    let Some(mut api_key) =
      self.select(ApiKeys::find_by_id(key)).into_model::<GetApiKey>().one(&txn).await?
    else {
      return Ok(None);
    };

    if api_key.is_secured {
      api_key.access_policies =
        Some(ap::for_users().user(ApUser::ApiKey(key.to_string())).find_all(&txn).await?);
    }

    txn.commit().await?;

    Ok(Some(api_key))
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
    &self,
    data: CreateApiKey,
  ) -> Result<InsertResult<entity::api_keys::ActiveModel>, SumidaError> {
    loop {
      let txn = db::get_connection().begin().await?;
      let key = match Self::generate_token_bytes(64) {
        Ok(v) => v,
        _ => {
          continue;
        }
      };

      let user_aps = if data.is_secured.0.unwrap_or_default() {
        data.access_policies.clone()
      } else {
        None
      };
      let mut active_model = data.clone().into_active_model();
      active_model.key = Set(key);

      match ApiKeys::insert(active_model).exec(&txn).await {
        Ok(result) => {
          if let Some(user_aps) = user_aps {
            ap::for_users()
              .user(ApUser::ApiKey(result.last_insert_id.clone()))
              .update(user_aps, &txn)
              .await?;
          }

          txn.commit().await?;
          return Ok(result);
        }
        Err(err) if Self::is_unique_violation(&err) => {
          txn.rollback().await?;
          continue;
        }
        Err(err) => {
          txn.rollback().await?;
          return Err(err.into());
        }
      }
    }
  }

  pub async fn update(
    &self,
    key: &str,
    data: UpdateApiKey,
  ) -> Result<entity::api_keys::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;

    self.ap().object(key).action("update").try_access(&txn).await?;

    let user_aps = if data.is_secured.0.unwrap_or_default() {
      data.access_policies.clone()
    } else {
      None
    };

    if let Some(user_aps) = user_aps {
      ap::for_users().user(ApUser::ApiKey(key.to_string())).update(user_aps, &txn).await?;
    }

    let mut model = data.into_active_model();
    model.key = Unchanged(key.to_string());

    let result = ApiKeys::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, key: &str) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().object(key).action("delete").try_access(&txn).await?;
    ap::for_users().user(ApUser::ApiKey(key.to_owned())).clear(&txn).await?;

    let result = ApiKeys::delete_by_id(key).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<ApCondition, ApiKeys> for ApiKeyService {
  fn select(&self, query: Select<ApiKeys>) -> Select<ApiKeys> {
    let user_alias = Alias::new("user");
    let col_id = Expr::col((user_alias.clone(), "id"));
    let col_username = Expr::col((user_alias.clone(), "username"));
    let col_full_name = Expr::col((user_alias.clone(), "full_name"));

    self.ap().action("read").apply(
      query
        .expr(Expr::col((entity::api_keys::Entity, Asterisk)))
        .expr_as(col_id, "user_id")
        .expr_as(col_username.clone(), "user_username")
        .expr_as(col_full_name.clone(), "user_full_name")
        .join_as(JoinType::LeftJoin, entity::api_keys::Relation::User.def(), user_alias.clone()),
    )
  }

  fn ap(&self) -> ApCondition {
    use entity::api_keys::Column;

    ap::ns("api_key") //
      .who(&self.session)
      .object_ref(Expr::col((ApiKeys, Column::Key)).cast_as("varchar"))
  }
}
