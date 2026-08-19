pub mod dto;

use error::SumidaError;
use sea_orm::ConnectionTrait;

pub use self::dto::*;

/// Reads that summarise the whole installation rather than one resource. Each answers a question a
/// list cannot: a list shows rows, these show shape.
pub struct DashboardService;

impl DashboardService {
  pub async fn auth_activity<C: ConnectionTrait>(db: &C) -> Result<AuthActivity, SumidaError> {
    Ok(AuthActivity::new(db).await?)
  }

  pub async fn account_activity<C: ConnectionTrait>(db: &C) -> Result<AccountActivity, SumidaError> {
    Ok(AccountActivity::new(db).await?)
  }

  pub async fn storage_usage<C: ConnectionTrait>(db: &C) -> Result<StorageUsage, SumidaError> {
    Ok(StorageUsage::new(db).await?)
  }
}
