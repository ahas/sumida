use tower_sessions_redis_store::{RedisStore, fred::prelude::*};

use crate::settings::session::SessionRedisSettings;

pub async fn new(settings: &SessionRedisSettings) -> Result<RedisStore<Pool>, anyhow::Error> {
  let config = Config::from_url(&settings.url).unwrap();
  let pool = Pool::new(config, None, None, None, 6)?;
  tokio::spawn(pool.connect());
  pool.wait_for_connect().await?;

  Ok(RedisStore::new(pool))
}
