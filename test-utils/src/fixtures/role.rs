use chrono::Utc;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;
use serde_json::json;

use sumida::entity::roles;
use sumida::entity::roles::{Ability, DefaultRoleAbility};

type RoleModel = roles::Model;
type RoleAModel = roles::ActiveModel;

// -- Utilities

fn to_json(value: &impl serde::Serialize) -> sea_orm::prelude::Json {
  Json::from_value(serde_json::to_value(value).unwrap())
}

// -- Model

pub fn mk_role(id: i32, name: impl Into<String>) -> RoleModel {
  let now: DateTimeWithTimeZone = Utc::now().into();
  let ability = DefaultRoleAbility::default();

  RoleModel {
    id,
    name: name.into(),
    description: Some("auto fixture".into()),
    ability: to_json(&ability),
    created_at: now,
    updated_at: now,
  }
}

// -- ActiveModel

pub fn mk_role_am(id: i32, name: impl Into<String>) -> RoleAModel {
  let now: DateTimeWithTimeZone = Utc::now().into();
  let ability = DefaultRoleAbility::default();

  RoleAModel {
    id: Set(id),
    name: Set(name.into()),
    description: Set(Some("auto fixture".into())),
    ability: Set(to_json(&ability)),
    created_at: Set(now),
    updated_at: Set(now),
  }
}
