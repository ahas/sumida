use api::service::prelude::*;
use api::util::random::generate_url_safe_string;
use entity::prelude::*;
use reqwest::Response;
use sea_orm::{ConnectionTrait, EntityTrait, Set};

use crate::common::prelude::*;

pub struct TestOrg {
  pub model: entity::organizations::Model,
}

impl TestOrg {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    let id = Organizations::insert(entity::organizations::ActiveModel {
      name: Set(format!("TestRole-{}", generate_url_safe_string(6).unwrap())),
      ..Default::default()
    })
    .exec(db)
    .await?
    .last_insert_id;

    Ok(Self {
      model: Organizations::find_by_id(id).one(db).await?.unwrap(),
    })
  }
}

pub struct TestRole {
  pub model: entity::roles::Model,
}

impl TestRole {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    let id = Roles::insert(entity::roles::ActiveModel {
      name: Set(format!("TestRole-{}", generate_url_safe_string(6).unwrap())),
      description: Set(Some(generate_url_safe_string(8).unwrap())),
      ..Default::default()
    })
    .exec(db)
    .await?
    .last_insert_id;

    Ok(Self {
      model: Roles::find_by_id(id).one(db).await?.unwrap(),
    })
  }
}

pub struct TestUser {
  pub model: entity::users::Model,
  pub password: String,
}

impl TestUser {
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    let org = TestOrg::create(db).await?;
    let role = TestRole::create(db).await?;
    let password = generate_url_safe_string(8).unwrap();
    let hashed_password = UserService::hash_password(password.clone()).expect("Failed to hash password");

    let id = Users::insert(entity::users::ActiveModel {
      username: Set(format!("test-{}", generate_url_safe_string(8).unwrap())),
      password: Set(hashed_password),
      email: Set(Some(generate_url_safe_string(8).unwrap())),
      full_name: Set(Some(generate_url_safe_string(8).unwrap())),
      phone: Set(Some(generate_url_safe_string(8).unwrap())),
      role_id: Set(role.model.id),
      organization_id: Set(org.model.id),
      is_admin: Set(true),
      is_enabled: Set(true),
      ..Default::default()
    })
    .exec(db)
    .await?
    .last_insert_id;

    Ok(Self {
      model: Users::find_by_id(id).one(db).await?.unwrap(),
      password,
    })
  }

  pub async fn login(&self, client: &TestClient<'_>) -> Response {
    let path = "/auth/sessions";
    client
      .post(path)
      .json(&LoginSession {
        username: self.model.username.clone(),
        password: self.password.clone(),
        remember_me: Some(false),
      })
      .send()
      .await
      .expect(&format!("Failed to send a POST request to {path}"))
  }
}
