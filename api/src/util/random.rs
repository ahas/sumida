use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;

pub fn generate_url_safe_string(entropy_bytes: usize) -> anyhow::Result<String> {
  let mut buf = vec![0u8; entropy_bytes];
  SysRng.try_fill_bytes(&mut buf)?;

  Ok(URL_SAFE_NO_PAD.encode(buf))
}
