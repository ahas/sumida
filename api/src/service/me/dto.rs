use axum::Json;
use axum::response::IntoResponse;
use entity::users::*;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::user::dto::{UserOrganization, UserRole};

/// The account behind a request, as it sees itself.
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Me {
  pub id:            i32,
  /// The name signed in with.
  pub username:      String,
  pub full_name:     Option<String>,
  pub email:         Option<String>,
  pub phone:         Option<String>,
  /// Whether the account is a platform administrator by its own column. A grant can confer the
  /// same reach without this being set.
  pub is_admin:      bool,
  /// Whether the account may be signed in to at all.
  pub is_enabled:    bool,
  /// Whether a live ban names this account. A ban can instead name an address or a device, and
  /// those are matched against the request rather than against the account — so this being `false`
  /// is not a promise that the next request gets through.
  #[sea_orm(from_alias = "is_banned")]
  pub is_banned:     bool,
  #[sea_orm(nested)]
  pub organization:  UserOrganization,
  #[sea_orm(nested)]
  pub role:          UserRole,
  /// When **this session** was authorized, not when the account was created.
  #[sea_orm(skip)]
  pub first_seen_at: DateTimeUtc,
  /// When this session was last used.
  #[sea_orm(skip)]
  pub last_seen_at:  DateTimeUtc,
  /// When the account row was created.
  pub created_at:    DateTimeUtc,
  /// When the account row was last changed.
  pub updated_at:    DateTimeUtc,
}

impl IntoResponse for Me {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

/// A change to one's own account.
///
/// Every key may be left out, and what leaving one out means differs by field. `username` and
/// `password` are left alone. `email`, `fullName` and `phone` are **cleared** — for those three,
/// absent and `null` are the same instruction, so a request meaning to change only one of them has
/// to send the current value of the other two.
///
/// A new `password`, or an `email` other than the one on file, is the change that takes an account
/// over — the password signs in, and the address is where its reset codes go — so it takes proof
/// beyond the credential the request carries: the `currentPassword`, or a `verificationCode` of kind
/// `account-change` sent to the account.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UpdateMe {
  /// Ignored. The account keeps the name it signed in with, so sending a different one changes
  /// nothing.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub username:          Optional<String>,
  /// A new password, hashed before it is stored and never read back by any route. Absent or `null`
  /// leaves the current one in place.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub password:          Optional<String>,
  /// Absent or `null` clears it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub email:             Nullable<String>,
  /// Absent or `null` clears it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub full_name:         Nullable<String>,
  /// Absent or `null` clears it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub phone:             Nullable<String>,
  /// The password the account has now. Proof for a change of `password` or `email`.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub current_password:  Optional<String>,
  /// A code of kind `account-change`, asked for with `POST /user-verifications`. Proof for a change
  /// of `password` or `email`, in place of the current password.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub verification_code: Optional<String>,
}

impl IntoActiveModel<ActiveModel> for UpdateMe {
  /// The account's own columns. The two proofs are not columns and are never written.
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      username: self.username.into_active_value(),
      password: self.password.into_active_value(),
      email: self.email.into_active_value(),
      full_name: self.full_name.into_active_value(),
      phone: self.phone.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeUpdated;

impl IntoResponse for MeUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
