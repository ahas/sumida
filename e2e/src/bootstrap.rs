use entity::prelude::*;
use sea_orm::{EntityTrait, Set};
use tokio::sync::OnceCell;

use crate::db::TestDb;

static INIT: OnceCell<()> = OnceCell::const_new();

pub async fn init_once() {
  INIT
    .get_or_init(|| async {
      dotenvy::from_filename(".env.e2e").expect("Failed to load .env.e2e");
      let settings = infra::settings::get();
      let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(settings.server.logging.as_deref().unwrap_or("trace"))
        .try_init();
      let db = infra::db::establish_connection().await;
      // TestDb::setup().await.ok();

      AccessPolicies::insert(entity::access_policies::ActiveModel {
        namespace: Set("*".to_string()),
        user: Set("user:admin".to_string()),
        object: Set("*".to_string()),
        action: Set(vec!["*".to_string()]),
        effect: Set("allow".to_string()),
        ..Default::default()
      })
      .exec(db)
      .await
      .expect("Failed to insert the admin access policy");
    })
    .await;
}
