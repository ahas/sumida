use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationContext {
  pub user_id:   i32,
  pub full_name: String,
  pub uuid:      String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationContext {
  pub code: String,
}
