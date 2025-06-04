use chrono::Utc;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;
use sumida::entity::organizations;

// -- Type aliases

type OrgModel = organizations::Model;
type OrgAModel = organizations::ActiveModel;

// -- Model

pub fn mk_org(id: i32, name: impl Into<String>, parent: Option<i32>) -> OrgModel {
  let now: DateTimeWithTimeZone = Utc::now().into();

  OrgModel {
    id,
    name: name.into(),
    parent,
    created_at: now,
    updated_at: now,
  }
}

// -- ActiveModel

pub fn mk_org_am(id: i32, name: impl Into<String>, parent: Option<i32>) -> OrgAModel {
  let now: DateTimeWithTimeZone = Utc::now().into();

  OrgAModel {
    id: Set(id),
    name: Set(name.into()),
    parent: Set(parent),
    created_at: Set(now),
    updated_at: Set(now),
  }
}
