pub mod s3;

use serde::Deserialize;

#[cfg(feature = "aws")]
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AwsSettings {
  pub region: Option<String>,
  #[cfg(feature = "aws-s3")]
  pub s3:     Option<s3::AwsS3Settings>,
}
