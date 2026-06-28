use cfg_if::cfg_if;
use chrono::{DateTime, Utc};
use error::SumidaError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenData {
  pub user_id: i32,
  pub username: String,
  pub jti: String,
  pub expires_at: DateTime<Utc>,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
  pub jti: String,
  pub expires_at: DateTime<Utc>,
  pub blacklisted_at: DateTime<Utc>,
}

/// Rust 1.75+ native `async fn` in trait. A trait object (`dyn JwtTokenStore`) is not possible, so
/// callers take it via `impl JwtTokenStore` or a generic param.
pub trait JwtTokenStore: Send + Sync {
  fn store_refresh_token(
    &self,
    token_id: &str,
    data: RefreshTokenData,
  ) -> impl Future<Output = Result<(), SumidaError>> + Send;
  fn get_refresh_token(
    &self,
    token_id: &str,
  ) -> impl Future<Output = Result<Option<RefreshTokenData>, SumidaError>> + Send;
  fn delete_refresh_token(&self, token_id: &str) -> impl Future<Output = Result<(), SumidaError>> + Send;
  fn blacklist_token(
    &self,
    jti: &str,
    expires_at: DateTime<Utc>,
  ) -> impl Future<Output = Result<(), SumidaError>> + Send;
  fn is_token_blacklisted(&self, jti: &str) -> impl Future<Output = Result<bool, SumidaError>> + Send;
  fn cleanup_expired(&self) -> impl Future<Output = Result<(), SumidaError>> + Send;
}

cfg_if! {
  if #[cfg(feature = "session-store-redis")] {
    use tower_sessions_redis_store::fred::prelude::*;
    use tower_sessions_redis_store::fred::types::Expiration;

    /// A fred (Redis)-backed JWT token store.
    /// - refresh tokens: stored as JSON under the `jwt:refresh:{token_id}` key (TTL until expiry time)
    /// - blacklist: determined by the existence of the `jwt:blacklist:{jti}` key (TTL until expiry time)
    pub struct RedisJwtStore {
      pool: Pool,
    }

    impl RedisJwtStore {
      pub async fn new(redis_url: &str) -> Result<Self, SumidaError> {
        let config =
          Config::from_url(redis_url).map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        let pool = Pool::new(config, None, None, None, 5)
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        pool.connect();
        pool.wait_for_connect().await.map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        Ok(Self { pool })
      }
    }

    impl JwtTokenStore for RedisJwtStore {
      async fn store_refresh_token(&self, token_id: &str, data: RefreshTokenData) -> Result<(), SumidaError> {
        let json = serde_json::to_string(&data).map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        let expire = Some(Expiration::EXAT(data.expires_at.timestamp()));
        self
          .pool
          .set::<(), _, _>(format!("jwt:refresh:{token_id}"), json, expire, None, false)
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        Ok(())
      }

      async fn get_refresh_token(&self, token_id: &str) -> Result<Option<RefreshTokenData>, SumidaError> {
        let value: Option<String> = self
          .pool
          .get(format!("jwt:refresh:{token_id}"))
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        match value {
          Some(json) => {
            Ok(Some(serde_json::from_str(&json).map_err(|e| SumidaError::Unsupported(e.to_string()))?))
          }
          None => Ok(None),
        }
      }

      async fn delete_refresh_token(&self, token_id: &str) -> Result<(), SumidaError> {
        self
          .pool
          .del::<(), _>(format!("jwt:refresh:{token_id}"))
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        Ok(())
      }

      async fn blacklist_token(&self, jti: &str, expires_at: DateTime<Utc>) -> Result<(), SumidaError> {
        let expire = Some(Expiration::EXAT(expires_at.timestamp()));
        self
          .pool
          .set::<(), _, _>(format!("jwt:blacklist:{jti}"), "1", expire, None, false)
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        Ok(())
      }

      async fn is_token_blacklisted(&self, jti: &str) -> Result<bool, SumidaError> {
        let count: i64 = self
          .pool
          .exists(format!("jwt:blacklist:{jti}"))
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;
        Ok(count > 0)
      }

      async fn cleanup_expired(&self) -> Result<(), SumidaError> {
        // Redis auto-expires via TTL, so no separate cleanup is needed.
        Ok(())
      }
    }
  } else if #[cfg(feature = "session-store-aws-dynamodb")] {
    use aws_sdk_dynamodb::{Client as DynamoClient, types::AttributeValue};
    use std::collections::HashMap;

    pub struct DynamoJwtStore {
      client: DynamoClient,
      table_name: String,
    }

    impl DynamoJwtStore {
      pub fn new(client: DynamoClient, table_name: String) -> Self {
        Self { client, table_name }
      }
    }

    impl JwtTokenStore for DynamoJwtStore {
      async fn store_refresh_token(&self, token_id: &str, data: RefreshTokenData) -> Result<(), SumidaError> {
        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(format!("refresh#{}", token_id)));
        item.insert("sk".to_string(), AttributeValue::S("data".to_string()));
        item.insert("user_id".to_string(), AttributeValue::N(data.user_id.to_string()));
        item.insert("username".to_string(), AttributeValue::S(data.username));
        item.insert("jti".to_string(), AttributeValue::S(data.jti));
        item.insert("expires_at".to_string(), AttributeValue::N(data.expires_at.timestamp().to_string()));
        item.insert("created_at".to_string(), AttributeValue::N(data.created_at.timestamp().to_string()));
        item.insert("ttl".to_string(), AttributeValue::N(data.expires_at.timestamp().to_string()));

        self.client
          .put_item()
          .table_name(&self.table_name)
          .set_item(Some(item))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(())
      }

      async fn get_refresh_token(&self, token_id: &str) -> Result<Option<RefreshTokenData>, SumidaError> {
        let key = HashMap::from([
          ("pk".to_string(), AttributeValue::S(format!("refresh#{}", token_id))),
          ("sk".to_string(), AttributeValue::S("data".to_string())),
        ]);

        let result = self.client
          .get_item()
          .table_name(&self.table_name)
          .set_key(Some(key))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        match result.item {
          Some(item) => {
            let user_id = item.get("user_id")
              .and_then(|v| v.as_n().ok())
              .and_then(|n| n.parse::<i32>().ok())
              .ok_or(SumidaError::Unsupported("Invalid user_id".to_string()))?;

            let username = item.get("username")
              .and_then(|v| v.as_s().ok())
              .ok_or(SumidaError::Unsupported("Invalid username".to_string()))?;

            let jti = item.get("jti")
              .and_then(|v| v.as_s().ok())
              .ok_or(SumidaError::Unsupported("Invalid jti".to_string()))?;

            let expires_at_ts = item.get("expires_at")
              .and_then(|v| v.as_n().ok())
              .and_then(|n| n.parse::<i64>().ok())
              .ok_or(SumidaError::Unsupported("Invalid expires_at".to_string()))?;

            let created_at_ts = item.get("created_at")
              .and_then(|v| v.as_n().ok())
              .and_then(|n| n.parse::<i64>().ok())
              .ok_or(SumidaError::Unsupported("Invalid created_at".to_string()))?;

            let data = RefreshTokenData {
              user_id,
              username: username.clone(),
              jti: jti.clone(),
              expires_at: DateTime::from_timestamp(expires_at_ts, 0).unwrap_or_default(),
              created_at: DateTime::from_timestamp(created_at_ts, 0).unwrap_or_default(),
            };

            Ok(Some(data))
          }
          None => Ok(None),
        }
      }

      async fn delete_refresh_token(&self, token_id: &str) -> Result<(), SumidaError> {
        let key = HashMap::from([
          ("pk".to_string(), AttributeValue::S(format!("refresh#{}", token_id))),
          ("sk".to_string(), AttributeValue::S("data".to_string())),
        ]);

        self.client
          .delete_item()
          .table_name(&self.table_name)
          .set_key(Some(key))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(())
      }

      async fn blacklist_token(&self, jti: &str, expires_at: DateTime<Utc>) -> Result<(), SumidaError> {
        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(format!("blacklist#{}", jti)));
        item.insert("sk".to_string(), AttributeValue::S("data".to_string()));
        item.insert("jti".to_string(), AttributeValue::S(jti.to_string()));
        item.insert("expires_at".to_string(), AttributeValue::N(expires_at.timestamp().to_string()));
        item.insert("blacklisted_at".to_string(), AttributeValue::N(Utc::now().timestamp().to_string()));
        item.insert("ttl".to_string(), AttributeValue::N(expires_at.timestamp().to_string()));

        self.client
          .put_item()
          .table_name(&self.table_name)
          .set_item(Some(item))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(())
      }

      async fn is_token_blacklisted(&self, jti: &str) -> Result<bool, SumidaError> {
        let key = HashMap::from([
          ("pk".to_string(), AttributeValue::S(format!("blacklist#{}", jti))),
          ("sk".to_string(), AttributeValue::S("data".to_string())),
        ]);

        let result = self.client
          .get_item()
          .table_name(&self.table_name)
          .set_key(Some(key))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(result.item.is_some())
      }

      async fn cleanup_expired(&self) -> Result<(), SumidaError> {
        // DynamoDB automatically expires items with TTL, so no manual cleanup needed
        Ok(())
      }
    }
  }
}

// DynamoDB JWT store integration test. Only built with the DynamoDB store enabled (redis off, since
// the store is selected by an if/else cfg), and only runs when SUMIDA_TEST_DYNAMO_TABLE points at a
// real table (pk/sk string keys + a `ttl` attribute). Requires working AWS credentials/region.
#[cfg(all(test, feature = "session-store-aws-dynamodb", not(feature = "session-store-redis")))]
mod dynamo_tests {
  use chrono::Utc;

  use super::*;

  #[tokio::test]
  async fn dynamo_jwt_store_roundtrip() {
    let Ok(table) = std::env::var("SUMIDA_TEST_DYNAMO_TABLE") else {
      eprintln!("skipping: set SUMIDA_TEST_DYNAMO_TABLE (+ AWS creds) to run the DynamoDB JWT store test");
      return;
    };

    let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = aws_sdk_dynamodb::Client::new(&cfg);
    let store = DynamoJwtStore::new(client, table);

    let jti = format!("test-{}", Utc::now().timestamp_micros());
    let expires_at = Utc::now() + chrono::Duration::hours(1);
    let data = RefreshTokenData {
      user_id: 42,
      username: "tester".to_string(),
      jti: jti.clone(),
      expires_at,
      created_at: Utc::now(),
    };

    // store -> get
    store.store_refresh_token(&jti, data).await.unwrap();
    let got = store.get_refresh_token(&jti).await.unwrap().expect("refresh token present");
    assert_eq!(got.user_id, 42);
    assert_eq!(got.jti, jti);

    // delete -> gone
    store.delete_refresh_token(&jti).await.unwrap();
    assert!(store.get_refresh_token(&jti).await.unwrap().is_none());

    // blacklist roundtrip
    assert!(!store.is_token_blacklisted(&jti).await.unwrap());
    store.blacklist_token(&jti, expires_at).await.unwrap();
    assert!(store.is_token_blacklisted(&jti).await.unwrap());
  }
}
