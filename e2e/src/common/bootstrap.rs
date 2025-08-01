use tokio::sync::OnceCell;

use crate::common::db::TestDb;

static INIT: OnceCell<()> = OnceCell::const_new();

pub async fn init_once() {
  INIT
    .get_or_init(|| async {
      dotenvy::from_filename(".env.e2e").expect("Failed to load .env.e2e");
      infra::settings::get();
      infra::db::establish_connection().await;
      TestDb::setup().await.ok();

      let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    })
    .await;
}
