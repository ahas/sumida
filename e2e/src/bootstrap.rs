use tokio::sync::OnceCell;

use crate::db::TestDb;

static INIT: OnceCell<()> = OnceCell::const_new();

pub async fn init_once() {
  INIT
    .get_or_init(|| async {
      dotenvy::from_filename(".env.e2e").expect("Failed to load .env.e2e");
      let settings = infra::settings::get();
      tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(settings.server.logging.as_deref().unwrap_or("trace"))
        .try_init()
        .ok();
      TestDb::setup().await.expect("Failed to setup test db");
    })
    .await;
}
