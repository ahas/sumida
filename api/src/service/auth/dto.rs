use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
  pub username:    String,
  pub password:    String,
  pub remember_me: Option<bool>,
}
