use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::str::FromStr;
use tokio::sync::OnceCell;

use crate::settings;

static DB: OnceCell<DatabaseConnection> = OnceCell::const_new();

pub async fn establish_connection() -> &'static DatabaseConnection {
  DB.get_or_init(|| async { connect().await }).await
}

pub fn get_connection() -> &'static DatabaseConnection {
  DB.get().unwrap()
}

pub async fn connect() -> DatabaseConnection {
  Database::connect(get_connect_options()).await.expect("Failed to connect database")
}

fn parse_to_duration(value: Option<&String>) -> Option<std::time::Duration> {
  value.and_then(|x| humantime::parse_duration(x).ok())
}

fn get_connect_options() -> ConnectOptions {
  let config = &settings::get().database;
  let connect_timeout = parse_to_duration(config.connect_timeout.as_ref());
  let idle_timeout = parse_to_duration(config.idle_timeout.as_ref());
  let acquire_timeout = parse_to_duration(config.acquire_timeout.as_ref());
  let max_lifetime = parse_to_duration(config.max_lifetime.as_ref());

  let mut conn_opts = ConnectOptions::new(&config.url);

  if let Some(v) = config.max_connections {
    conn_opts.max_connections(v);
  }
  if let Some(v) = config.min_connections {
    conn_opts.min_connections(v);
  }
  if let Some(v) = connect_timeout {
    conn_opts.connect_timeout(v);
  }
  if let Some(v) = idle_timeout {
    conn_opts.idle_timeout(v);
  }
  if let Some(v) = acquire_timeout {
    conn_opts.acquire_timeout(v);
  }
  if let Some(v) = max_lifetime {
    conn_opts.max_lifetime(v);
  }

  let logging = config.logging.clone().unwrap_or("off".to_string());
  let is_logging_enabled = logging != "off" && logging != "false";
  conn_opts.sqlx_logging(is_logging_enabled);

  if is_logging_enabled {
    let filter = tracing::log::LevelFilter::from_str(&logging).unwrap();
    conn_opts.sqlx_logging_level(filter);
  }

  conn_opts
}
