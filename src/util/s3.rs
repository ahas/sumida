use crate::core::{aws, settings};

use anyhow::anyhow;
use aws_sdk_s3::primitives::{AggregatedBytes, ByteStream};
#[cfg(feature = "image")]
use image::{self, DynamicImage, ImageFormat};
use std::{error::Error, fmt::Display, io::Cursor};
use tokio::io::AsyncReadExt;

pub trait IntoS3Uri {
  fn into_s3_uri(self) -> String;
}

impl IntoS3Uri for (String, String) {
  fn into_s3_uri(self) -> String {
    format!("s3://{bucket}/{key}", bucket = &self.0, key = &self.1)
  }
}

#[allow(unused)]
pub fn new_client() -> aws_sdk_s3::Client {
  aws_sdk_s3::Client::new(aws::get_sdk_config())
}

#[allow(unused)]
pub fn static_uri(data_type: impl Display, id: impl Display, path: impl Display) -> String {
  format!(
    "s3://{bucket}/static/{stage}/{data_type}/{id}/{path}",
    bucket = get_bucket().unwrap(),
    stage = &settings::get().server.stage
  )
}

#[allow(unused)]
pub fn get_bucket() -> Result<String, anyhow::Error> {
  let settings = settings::get();
  let ret = settings
    .aws
    .as_ref()
    .and_then(|x| x.s3.as_ref())
    .map(|x| x.bucket.clone())
    .ok_or_else(|| anyhow!("aws s3 bucket is not set"));

  ret
}

#[allow(unused)]
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

#[allow(unused)]
pub async fn get_bytes(client: &aws_sdk_s3::Client, uri: String) -> Result<AggregatedBytes, anyhow::Error> {
  let (bucket, key) = parse_uri(uri)?;
  let resp = client.get_object().bucket(&bucket).key(&key).send().await?;

  Ok(resp.body.collect().await?)
}

#[allow(unused)]
pub async fn get_json(
  client: &aws_sdk_s3::Client,
  uri: String,
) -> Result<serde_json::Value, anyhow::Error> {
  let bytes = get_bytes(client, uri.clone()).await?;

  serde_json::from_slice(&bytes.into_bytes())
    .map_err(|err| anyhow!("json parsing failure {uri}: {}", err.to_string()))
}

#[allow(unused)]
#[cfg(feature = "image")]
pub async fn get_image(client: &aws_sdk_s3::Client, uri: String) -> Option<DynamicImage> {
  let bytes = get_bytes(client, uri.clone()).await.ok();

  bytes.map(|x| x.into_bytes()).map(Cursor::new).and_then(|x| {
    let format = ImageFormat::from_path(uri).unwrap();
    image::load(x, format).ok()
  })
}

#[allow(unused)]
#[cfg(feature = "image")]
pub async fn put_image(
  client: &aws_sdk_s3::Client,
  uri: String,
  body: ByteStream,
) -> Result<(), anyhow::Error> {
  let (bucket, key) = parse_uri(uri).unwrap();
  let image_format = ImageFormat::from_path(&key).unwrap();

  client
    .put_object()
    .bucket(bucket)
    .key(key)
    .content_type(image_format.to_mime_type())
    .body(body)
    .send()
    .await;

  Ok(())
}

#[allow(unused)]
pub async fn exists(client: &aws_sdk_s3::Client, uri: String) -> bool {
  let (bucket, key) = parse_uri(uri).unwrap();
  let resp = client.head_object().bucket(bucket).key(key).send().await.ok();

  resp.is_some()
}

#[allow(unused)]
pub async fn delete(client: &aws_sdk_s3::Client, uri: String) -> Result<(), anyhow::Error> {
  let (bucket, key) = parse_uri(uri)?;
  client.delete_object().bucket(bucket).key(key).send().await?;

  Ok(())
}
