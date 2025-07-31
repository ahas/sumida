use lazy_static::lazy_static;
use serde::Deserialize;

lazy_static! {
  pub static ref WEB_PUSH_PRIVATE_KEY_PEM: jsonwebtoken::EncodingKey = {
    let pem = std::fs::read_to_string(std::path::Path::new(&super::get().web_push.private_key))
      .expect("failed to read web-push private key file");

    jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes())
      .expect("failed to parse web-push private key file to jwt encoding key")
  };
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WebPushSettings {
  pub audience: String,
  pub expiration: String,
  pub subject: String,
  pub private_key: String,
}
