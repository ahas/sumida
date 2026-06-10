pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Unchanged;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{
  DbErr, DeleteResult, EntityTrait, IntoActiveModel, IntoSimpleExpr, JoinType, PaginatorTrait,
  QuerySelect, RelationTrait, Select, Set, SqlErr, TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard, ApUser};
use crate::util::pagination::Pagination;

/// Converts an API key into a hash for storage and lookup. The plaintext is exposed only once at issuance; the DB keeps only the hash.
/// (Prevents key reuse on a DB leak. Must use the same algorithm as the migration's conversion logic.)
pub fn hash_api_key(key: &str) -> String {
  blake3::hash(key.as_bytes()).to_hex().to_string()
}

pub struct ApiKeyService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> ApiKeyService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    ApiKeyService { state, session }
  }

  pub async fn list(
    &self,
    filter: ApiKeyFilter,
    pagination: Pagination,
  ) -> Result<(Vec<ApiKeyListItem>, u64), SumidaError> {
    use entity::api_keys::Column;

    let db = self.state.db();
    let col_username = Expr::col("user_username");
    let col_full_name = Expr::col("user_full_name");

    let mut query = filter.apply(self.select(ApiKeys::find()));
    let count = query.clone().select_only().column(Column::Key).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "key" => Column::Key.into_simple_expr(),
      "username" => col_username.into_simple_expr(),
      "name" => col_full_name.into_simple_expr(),
      _ => Column::Key.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<ApiKeyListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, key: &str) -> Result<Option<GetApiKey>, SumidaError> {
    let txn = self.state.db().begin().await?;

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

  fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
  }

  /// Issues a new API key. The plaintext key is exposed **only once, via this return value**, and the DB stores only the hash.
  /// Policies (access_policies) are also bound to the hash identifier (`api:<hash>`) (looked up by hash on verify).
  pub async fn create(&self, data: CreateApiKey) -> Result<String, SumidaError> {
    loop {
      let txn = self.state.db().begin().await?;
      let plaintext = match crate::util::random::generate_url_safe_string(64) {
        Ok(v) => v,
        _ => {
          continue;
        }
      };
      let key_hash = hash_api_key(&plaintext);
      // Identifying prefix (first 8 chars of the plaintext). Not a secret; for display in the list UI.
      let key_prefix: String = plaintext.chars().take(8).collect();

      let user_aps =
        if data.is_secured.0.unwrap_or_default() { data.access_policies.clone() } else { None };
      let mut active_model = data.clone().into_active_model();
      active_model.key = Set(key_hash.clone());
      active_model.key_prefix = Set(Some(key_prefix));

      match ApiKeys::insert(active_model).exec(&txn).await {
        Ok(_) => {
          if let Some(user_aps) = user_aps {
            for policy in &user_aps {
              ap::assert_policy_within_authority(
                self.session.profile.is_admin,
                &policy.namespace,
                &policy.object,
                &policy.action,
                "",
                &policy.effect,
              )?;
            }
            ap::for_users().user(ApUser::ApiKey(key_hash.clone())).update(user_aps, &txn).await?;
          }

          txn.commit().await?;
          // The plaintext is returned only here (not retrievable afterwards).
          return Ok(plaintext);
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
    let txn = self.state.db().begin().await?;

    self.try_access("update", ApiKeys::find_by_id(key), &txn).await?;

    let user_aps = if data.is_secured.0.unwrap_or_default() { data.access_policies.clone() } else { None };

    if let Some(user_aps) = user_aps {
      for policy in &user_aps {
        ap::assert_policy_within_authority(
          self.session.profile.is_admin,
          &policy.namespace,
          &policy.object,
          &policy.action,
          "",
          &policy.effect,
        )?;
      }
      ap::for_users().user(ApUser::ApiKey(key.to_string())).update(user_aps, &txn).await?;
    }

    let mut model = data.into_active_model();
    model.key = Unchanged(key.to_string());

    let result = ApiKeys::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, key: &str) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", ApiKeys::find_by_id(key), &txn).await?;
    ap::for_users().user(ApUser::ApiKey(key.to_owned())).clear(&txn).await?;

    let result = ApiKeys::delete_by_id(key).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, ApiKeys> for ApiKeyService<S> {
  fn default_query_with(query: Select<ApiKeys>) -> Select<ApiKeys> {
    let user_alias = Alias::new("user");
    let col_id = Expr::col((user_alias.clone(), "id"));
    let col_username = Expr::col((user_alias.clone(), "username"));
    let col_full_name = Expr::col((user_alias.clone(), "full_name"));

    query
      .expr(Expr::col((entity::api_keys::Entity, Asterisk)))
      .expr_as(col_id, "user_id")
      .expr_as(col_username.clone(), "user_username")
      .expr_as(col_full_name.clone(), "user_full_name")
      .join_as(JoinType::LeftJoin, entity::api_keys::Relation::User.def(), user_alias.clone())
  }

  fn ap(&self) -> ApCondition {
    use entity::api_keys::Column;
    ap::ns("api_key").who(&self.session).join(ApiKeys, Column::Key)
  }

  fn select(&self, query: sea_orm::Select<ApiKeys>) -> sea_orm::Select<ApiKeys> {
    use entity::api_keys::Column;
    self
      .ap()
      .expr(Expr::col((ApiKeys, Column::Key)).cast_as("varchar"))
      .action("read")
      .apply(Self::default_query_with(query))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // hash_api_key (M-2): deterministic, 64-char hex, and different inputs yield different hashes
  #[test]
  fn hash_is_deterministic_hex64() {
    let h1 = hash_api_key("super-secret-key");
    let h2 = hash_api_key("super-secret-key");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert!(h1.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(hash_api_key("key-a"), hash_api_key("key-b"));
  }
}
