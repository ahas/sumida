use serde::Deserialize;

#[cfg(feature = "aws-s3")]
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AwsS3Settings {
  pub bucket: String,
}

