use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AwsS3Settings {
  pub bucket: String,
}
