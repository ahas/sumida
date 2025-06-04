use chrono::Utc;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;

use sumida::entity::users;

// -- Model

pub fn mk_user(
  username: impl Into<String>,
  password: impl AsRef<str>,
  org_id: i32,
  role_id: i32,
) -> users::Model {
  let now: DateTimeWithTimeZone = Utc::now().into();

  users::Model {
    username: username.into(),
    password: users::Entity::hash_password(password).unwrap(),
    organization: org_id,
    email: Some("alice@example.com".into()),
    full_name: Some("Alice".into()),
    phone: None,
    role: role_id,
    is_admin: false,
    is_enabled: true,
    last_sign_in_at: None,
    created_at: now,
    updated_at: now,
  }
}

// -- ActiveModel

pub fn mk_user_am(
  username: impl Into<String>,
  password: impl AsRef<str>,
  org_id: i32,
  role_id: i32,
) -> users::ActiveModel {
  let now: DateTimeWithTimeZone = Utc::now().into();

  users::ActiveModel {
    username: Set(username.into()),
    password: Set(users::Entity::hash_password(password).unwrap()),
    organization: Set(org_id),
    email: Set(Some("alice@example.com".into())),
    full_name: Set(Some("Alice".into())),
    phone: Set(None),
    role: Set(role_id),
    is_admin: Set(false),
    is_enabled: Set(true),
    last_sign_in_at: Set(None),
    created_at: Set(now),
    updated_at: Set(now),
  }
}
