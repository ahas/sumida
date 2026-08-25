use serde::{Deserialize, Serialize};

/// The credentials both ways in read: the session route and the token route take the same body.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
  /// The account name. Not an address unless the deployment's accounts are named by one.
  pub username:    String,
  /// The password in the clear, which is why this only ever travels over TLS.
  pub password:    String,
  /// How long the sign-in outlives the moment. Leaving it out is **not** the same as `false`:
  /// omitted, the sign-in lasts as long as the client keeps it; `false` gives it the deployment's
  /// ordinary window; `true` gives it the longer remembered one.
  pub remember_me: Option<bool>,
}
