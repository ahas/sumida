use crate::core::settings;

use tower_sessions::cookie::SameSite;
use tower_sessions::SessionManagerLayer;

use super::settings::SessionSettings;

#[cfg(feature = "session-store-aws-dynamodb")]
use tower_sessions_dynamodb_store::{DynamoDBStore, DynamoDBStoreProps};

#[cfg(feature = "session-store-redis")]
use tower_sessions_redis_store::{fred::prelude::*, RedisStore};

#[cfg(feature = "session-store-aws-dynamodb")]
async fn new_store(settings: &SessionSettings) -> Result<DynamoDBStore, anyhow::Error> {
  tracing::info!("- table: {}", settings.table);

  let client = aws_sdk_dynamodb::Client::new(crate::core::aws::get_sdk_config());
  let store_props = DynamoDBStoreProps {
    table_name: (settings.table).clone(),
    ..Default::default()
  };

  Ok(DynamoDBStore::new(client, store_props))
}

#[cfg(feature = "session-store-redis")]
async fn new_store(_settings: &SessionSettings) -> Result<RedisStore<Pool>, anyhow::Error> {
  let pool = Pool::new(Config::default(), None, None, None, 6)?;
  let conn = pool.connect();
  pool.wait_for_connect().await?;
  conn.await??;

  Ok(RedisStore::new(pool))
}

#[cfg(feature = "session-store-aws-dynamodb")]
type Store = DynamoDBStore;

#[cfg(feature = "session-store-redis")]
type Store = RedisStore<Pool>;

pub async fn new_layer() -> Result<SessionManagerLayer<Store>, anyhow::Error> {
  let settings = &settings::get().session;
  let name = settings.name.clone().unwrap_or(String::from("sid"));
  let secure = settings.secure.unwrap_or(true);
  let domain = settings.domain.clone();
  let path = settings.path.clone();
  let same_site = settings.same_site.clone().unwrap_or("none".to_string());

  tracing::info!("session layer settings:");
  tracing::info!("- name: {name}");
  tracing::info!("- secure: {secure}");
  tracing::info!("- domain: {:?}", domain);

  let store = new_store(settings).await?;

  let mut layer = SessionManagerLayer::new(store).with_name(name).with_secure(secure);

  if let Some(domain) = domain {
    layer = layer.with_domain(domain);
  }
  if let Some(path) = path {
    layer = layer.with_path(path);
  }
  let layer = match same_site.as_str() {
    "lax" => layer.with_same_site(SameSite::Lax),
    "strict" => layer.with_same_site(SameSite::Strict),
    _ => layer.with_same_site(SameSite::None),
  };

  Ok(layer)
}
