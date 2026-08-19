pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Unchanged;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{
  DbErr, DeleteResult, EntityTrait, IntoActiveModel, JoinType, PaginatorTrait, QuerySelect,
  RelationTrait, Select, Set, SqlErr, TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::authz::write::SubjectKey;
use crate::service::grants::{from_service_grants, to_service_grants};
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace api_key service = api_key`).
const RESOURCE: &str = "api_key";

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

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 키가 부여를 **받는** 쪽으로 등장할 때의 주체 키. 종전 `access_policies."user"` 의
  /// `api:<hash>` 와 같은 해시다.
  fn holder(key_hash: &str) -> SubjectKey {
    SubjectKey::apikey(key_hash)
  }

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

  /// 행 스코프는 `api_key#view` 술어 하나 — 키 소유자 본인과 그 사람을 관리하는 조직이 규칙
  /// 한 줄에 함께 들어 있다.
  fn select(&self, query: Select<ApiKeys>) -> Result<Select<ApiKeys>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `ApiKeyFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: ApiKeyFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<ApiKeyListItem>, u64), SumidaError> {
    use entity::api_keys::Column;

    let db = self.state.db();

    let mut query =
      conditions.apply::<ApiKeyFilter, _>(filter.apply(self.select(ApiKeys::find())?))?;
    let count = query.clone().select_only().column(Column::Key).count(db).await?;

    query = pagination.order_by_schema::<ApiKeyFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<ApiKeyListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, key: &str) -> Result<Option<GetApiKey>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(mut api_key) =
      self.select(ApiKeys::find_by_id(key))?.into_model::<GetApiKey>().one(&txn).await?
    else {
      return Ok(None);
    };

    if api_key.is_secured {
      // 보안 키가 보유한 라우트 게이트. 화면이 늘 읽어 온 모양 그대로다(§9.4).
      let held = SubjectSet::from_parts(vec![Self::holder(key).key()], Vec::new());
      api_key.access_policies =
        Some(from_service_grants(Authz::service_grants(&held, &txn).await?));
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
    // 발급된 키로 인증하면 세션이 키 소유자의 권한을 그대로 상속하므로(layer::user_guard::verify_api_key),
    // 타인 명의 발급을 허용하면 `api_key:create` 권한만으로 그 계정을 탈취할 수 있다.
    // 세션 사용자로 조용히 덮어쓰지 않고 거부해, 호출자가 잘못된 요청임을 알 수 있게 한다.
    if !self.session.profile.is_admin && data.user_id != self.session.profile.id {
      return Err(SumidaError::NoAccessPermission);
    }

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
            // 권한 검사는 `replace_service_grants` 안에서 네임스페이스마다
            // `service:<namespace>#grant` 로 이뤄진다.
            let grants = to_service_grants(&user_aps)?;
            Authz::replace_service_grants(
              &Self::holder(&key_hash),
              &grants,
              &self.subject(),
              &txn,
            )
            .await?;
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

    Authz::try_check(RESOURCE, key, "edit", &self.subject(), &txn).await?;

    let user_aps = if data.is_secured.0.unwrap_or_default() { data.access_policies.clone() } else { None };

    if let Some(user_aps) = user_aps {
      let grants = to_service_grants(&user_aps)?;
      Authz::replace_service_grants(&Self::holder(key), &grants, &self.subject(), &txn).await?;
    }

    let mut model = data.into_active_model();
    model.key = Unchanged(key.to_string());

    let result = ApiKeys::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, key: &str) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, key, "remove", &self.subject(), &txn).await?;
    // 키가 사라지면 그 키 앞으로 남은 부여도 전부 사라진다(§1.6).
    Authz::revoke_all_of(&Self::holder(key), &txn).await?;

    let result = ApiKeys::delete_by_id(key).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
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
