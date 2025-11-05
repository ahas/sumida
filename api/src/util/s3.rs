use std::fmt::Display;
use std::io::Cursor;

use anyhow::anyhow;
use aws_smithy_types::byte_stream::{AggregatedBytes, ByteStream};
#[cfg(feature = "image")]
use image::{self, DynamicImage, ImageFormat};
use infra::{aws, settings};

pub trait IntoS3Uri {
  fn into_s3_uri(self) -> String;
}

impl IntoS3Uri for (String, String) {
  fn into_s3_uri(self) -> String {
    format!("s3://{bucket}/{key}", bucket = &self.0, key = &self.1)
  }
}

#[allow(dead_code)]
pub fn new_client() -> aws_sdk_s3::Client {
  aws_sdk_s3::Client::new(aws::get_sdk_config())
}

#[allow(dead_code)]
pub fn static_uri(
  data_type: impl Display,
  stage: impl Display,
  id: impl Display,
  path: impl Display,
) -> String {
  format!(
    "s3://{bucket}/static/{stage}/{data_type}/{id}/{path}",
    bucket = get_bucket().unwrap_or_default(),
  )
}

#[allow(dead_code)]
pub fn get_bucket() -> Result<String, anyhow::Error> {
  settings::get()
    .aws
    .as_ref()
    .and_then(|x| x.s3.as_ref())
    .map(|x| x.bucket.clone())
    .ok_or_else(|| anyhow!("aws s3 bucket is not set"))
}

#[allow(dead_code)]
pub fn parse_uri(uri: String) -> Result<(String, String), anyhow::Error> {
  if let Some(stripped) = uri.strip_prefix("s3://") {
    if let Some(slash_pos) = stripped.find('/') {
      let bucket = &stripped[..slash_pos];
      let key = &stripped[slash_pos + 1..];

      Ok((bucket.to_string(), key.to_string()))
    } else {
      Err(anyhow!("invalid s3 uri"))
    }
  } else {
    Ok((get_bucket()?, uri.clone()))
  }
}

#[allow(dead_code)]
pub async fn get_bytes(client: &aws_sdk_s3::Client, uri: String) -> Result<AggregatedBytes, anyhow::Error> {
  let (bucket, key) = parse_uri(uri)?;
  let resp = client.get_object().bucket(&bucket).key(&key).send().await?;

  Ok(resp.body.collect().await?)
}

#[allow(dead_code)]
pub async fn get_json(
  client: &aws_sdk_s3::Client,
  uri: String,
) -> Result<serde_json::Value, anyhow::Error> {
  let bytes = get_bytes(client, uri.clone()).await?;

  serde_json::from_slice(&bytes.into_bytes())
    .map_err(|err| anyhow!("json parsing failure {uri}: {}", err.to_string()))
}

#[allow(dead_code)]
#[cfg(feature = "image")]
pub async fn get_image(client: &aws_sdk_s3::Client, uri: String) -> Result<DynamicImage, anyhow::Error> {
  let bytes = get_bytes(client, uri.clone()).await?;
  let cursor = Cursor::new(bytes.into_bytes());

  ImageFormat::from_path(&uri)
    .and_then(|x| image::load(cursor, x))
    .map_err(anyhow::Error::from)
}

#[allow(dead_code)]
#[cfg(feature = "image")]
pub async fn put_image(
  client: &aws_sdk_s3::Client,
  uri: String,
  body: ByteStream,
) -> Result<(), anyhow::Error> {
  let (bucket, key) = parse_uri(uri)?;
  let image_format = ImageFormat::from_path(&key).map_err(|e| anyhow!(e.to_string()))?;

  client
    .put_object()
    .bucket(bucket)
    .key(key)
    .content_type(image_format.to_mime_type())
    .body(body)
    .send()
    .await?;

  Ok(())
}

#[allow(dead_code)]
pub async fn exists(client: &aws_sdk_s3::Client, uri: String) -> Result<bool, anyhow::Error> {
  let Ok((bucket, key)) = parse_uri(uri) else {
    return Ok(false);
  };

  match client.head_object().bucket(bucket).key(key).send().await {
    Ok(_) => Ok(true),
    Err(err) => match err.into_service_error() {
      aws_sdk_s3::operation::head_object::HeadObjectError::NotFound(_) => Ok(false),
      other => Err(anyhow::Error::from(other)),
    },
  }
}

#[allow(dead_code)]
pub async fn delete(client: &aws_sdk_s3::Client, uri: String) -> Result<(), anyhow::Error> {
  let (bucket, key) = parse_uri(uri)?;
  client.delete_object().bucket(bucket).key(key).send().await?;

  Ok(())
}
