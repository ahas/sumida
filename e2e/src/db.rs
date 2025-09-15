use anyhow::Context;
use entity::prelude::*;
use sea_orm::{Database, DbBackend, Set, Statement, prelude::*};

#[allow(dead_code)]
pub struct TestDb;

impl TestDb {
  pub async fn setup() -> Result<Self, anyhow::Error> {
    let db = infra::db::connect().await;

    Self::find_or_create().await?;
    Self::run_migration_fresh(&db).await?;

    AccessPolicies::insert(entity::access_policies::ActiveModel {
      namespace: Set("*".to_string()),
      user: Set("user:admin".to_string()),
      object: Set("*".to_string()),
      action: Set(vec!["*".to_string()]),
      effect: Set("allow".to_string()),
      ..Default::default()
    })
    .exec(&db)
    .await
    .expect("Failed to insert the admin access policy");

    db.close().await?;

    Ok(Self)
  }

  async fn find_or_create() -> anyhow::Result<String> {
    let settings = infra::settings::get();
    let admin_url = std::env::var("DATABASE_ADMIN_URL").context("Missing env: DATABASE_ADMIN_URL")?;
    let test_url = settings.database.url.clone();

    let db_name = test_url //
      .split('/')
      .next_back()
      .ok_or_else(|| anyhow::anyhow!("Invalid DATABASE_URL: missing DB name"))?;

    // 1. Connect to admin DB
    let admin = Database::connect(&admin_url).await.context("Failed to connect to admin DB")?;

    // 2. Create test DB if it doesn't exist
    let exists = admin
      .query_one(Statement::from_string(
        DbBackend::Postgres,
        format!("SELECT 1 FROM pg_database WHERE datname = '{db_name}'"),
      ))
      .await?
      .is_some();

    if !exists {
      admin
        .execute(Statement::from_string(DbBackend::Postgres, format!("CREATE DATABASE {db_name}")))
        .await
        .context("Failed to create test database")?;

      tracing::info!("✅ Created database `{db_name}`");
    } else {
      tracing::info!("ℹ️ Database `{db_name}` already exists");
    }

    Ok(db_name.to_string())
  }

  #[allow(dead_code)]
  async fn run_migration_up(db: &DatabaseConnection) -> anyhow::Result<()> {
    use crate::migration::Migrator;
    use sea_orm_migration::prelude::*;

    Migrator::up(db, None).await.expect("Migration up command failed");

    tracing::info!("✅ Applied pending migrations");

    Ok(())
  }

  /// Use this method only when you need to completely reset the database
  #[allow(dead_code)]
  pub async fn reset_database() -> anyhow::Result<()> {
    let db = infra::db::connect().await;
    Self::run_migration_fresh(&db).await?;
    
    // Re-insert admin policy
    AccessPolicies::insert(entity::access_policies::ActiveModel {
      namespace: Set("*".to_string()),
      user: Set("user:admin".to_string()),
      object: Set("*".to_string()),
      action: Set(vec!["*".to_string()]),
      effect: Set("allow".to_string()),
      ..Default::default()
    })
    .exec(&db)
    .await
    .expect("Failed to insert the admin access policy");

    db.close().await?;
    Ok(())
  }

  async fn run_migration_fresh(db: &DatabaseConnection) -> anyhow::Result<()> {
    use crate::migration::Migrator;
    use sea_orm_migration::prelude::*;

    Migrator::fresh(db).await.expect("Migration fresh command failed");

    tracing::info!("✅ Applied fresh migrations");

    Ok(())
  }
}
