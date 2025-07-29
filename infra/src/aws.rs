use aws_config::{BehaviorVersion, meta::region::RegionProviderChain, retry::RetryConfig};
use tokio::sync::OnceCell;

static AWS_SDK_CONFIG: OnceCell<aws_config::SdkConfig> = OnceCell::const_new();

pub async fn load_sdk_config() {
  let region_provider = RegionProviderChain::default_provider();

  AWS_SDK_CONFIG
    .get_or_init(|| async {
      aws_config::defaults(BehaviorVersion::latest())
        .region(region_provider)
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .load()
        .await
    })
    .await;
}

pub fn get_sdk_config() -> &'static aws_config::SdkConfig {
  AWS_SDK_CONFIG.get().unwrap()
}
