use std::io::{BufWriter, Cursor};

use axum::http::header;
use axum::response::IntoResponse;
use image::{codecs::jpeg, imageops, DynamicImage, ImageFormat, RgbImage};

#[derive(Clone, Debug)]
pub struct ImageResponse(pub DynamicImage, pub ImageFormat);

impl IntoResponse for ImageResponse {
  fn into_response(self) -> axum::response::Response {
    let img = self.0;
    let format = self.1;

    let mut buffer = BufWriter::new(Cursor::new(Vec::new()));
    img.write_to(&mut buffer, format).expect("failed to write a image to buffer");
    let bytes = buffer.into_inner().map(|x| x.into_inner()).expect("failed to convert a image into byte");

    ([(header::CONTENT_TYPE, format.to_mime_type().to_string())], bytes).into_response()
  }
}

pub fn encode_to_jpeg(img: &DynamicImage, quality: u8) -> Vec<u8> {
  let mut buffer = Vec::new();
  let mut writer = Cursor::new(&mut buffer);
  let encoder = jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
  if let Err(err) = img.write_with_encoder(encoder) {
    tracing::error!("failed to encode jpeg: {}", err);
  }

  buffer
}

pub fn brighter(img: &RgbImage) -> RgbImage {
  let alpha: f32 = 0.5;
  let white = (1.0 - alpha) * 255.0;
  let mut img = img.clone();

  img.as_flat_samples_mut().samples.chunks_mut(3).for_each(|x| {
    x[0] = (x[0] as f32 * alpha + white).clamp(0.0, 255.0) as u8;
    x[1] = (x[1] as f32 * alpha + white).clamp(0.0, 255.0) as u8;
    x[2] = (x[2] as f32 * alpha + white).clamp(0.0, 255.0) as u8;
  });

  img
}

pub fn grayscale(img: &RgbImage) -> DynamicImage {
  imageops::colorops::grayscale(img).into()
}

pub fn downsize(img: &RgbImage) -> DynamicImage {
  let w: f64 = 320.0;
  let h = ((img.height() as f64) * (w / img.width() as f64)) as u32;
  let w = w as u32;

  imageops::resize(img, w, h, imageops::FilterType::Gaussian).into()
}
