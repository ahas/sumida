use tower_sessions_redis_store::{RedisStore, fred::prelude::*};

use crate::settings::session::SessionSettings;

pub async fn new(_settings: &SessionSettings) -> Result<RedisStore<Pool>, anyhow::Error> {
  let pool = Pool::new(Config::default(), None, None, None, 6)?;
  tokio::spawn(pool.connect());
  pool.wait_for_connect().await?;

  Ok(RedisStore::new(pool))
}
