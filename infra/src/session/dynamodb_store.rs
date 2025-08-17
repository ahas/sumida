use tower_sessions_dynamodb_store::{DynamoDBStore, DynamoDBStoreProps};

use crate::settings::session::SessionDynamodbSettings;

pub async fn new(settings: &SessionDynamodbSettings) -> Result<DynamoDBStore, anyhow::Error> {
  let client = aws_sdk_dynamodb::Client::new(crate::aws::get_sdk_config());
  let store_props = DynamoDBStoreProps {
    table_name: (settings.table).clone(),
    ..Default::default()
  };

  Ok(DynamoDBStore::new(client, store_props))
}
