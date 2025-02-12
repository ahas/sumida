use std::io::{BufWriter, Cursor};

use image::{codecs::jpeg, imageops, DynamicImage, ImageFormat, RgbImage};

pub fn encode_to_jpeg(img: &DynamicImage, quality: u8) -> Vec<u8> {
  let mut buffer = Vec::new();
  let mut writer = Cursor::new(&mut buffer);
  let mut encoder = jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
  img.write_with_encoder(encoder).unwrap();

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
