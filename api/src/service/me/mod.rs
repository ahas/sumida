pub mod dto;

use infra::db;
use infra::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  ActiveModelTrait,
  ActiveValue::{NotSet, Set, Unchanged},
  EntityTrait, IntoActiveModel, Select,
};
use sea_orm_crud::Optional;
use serde_json::json;

use dto::*;

use crate::service::ApiService;
use crate::service::user::UserService;
use crate::util::ap::Ap;

pub struct MeService {
  user_ctx: UserContext,
  user_service: UserService,
}

impl MeService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self {
      user_service: UserService::new(user_ctx.clone()),
      user_ctx,
    }
  }

  pub async fn get_session(&self) -> Result<Option<Me>, SumidaError> {
    if !self.user_ctx.profile.is_authorized {
      return Err(SumidaError::NoCredentials);
    }

    let Some(mut me_session) = self
      .select(Users::find_by_id(self.user_ctx.profile.id))
      .into_model::<Me>()
      .one(db::get_connection())
      .await?
    else {
      return Ok(None);
    };

    me_session.first_seen_at = self.user_ctx.profile.first_seen_at;
    me_session.last_seen_at = self.user_ctx.profile.last_seen_at;

    Ok(Some(me_session))
  }

  pub async fn update_me(&mut self, data: UpdateMe) -> Result<entity::users::Model, SumidaError> {
    let username = &self.user_ctx.profile.username;
    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.id = Unchanged(self.user_ctx.profile.id);
    model.username = Set(username.clone());

    if let Some(ref password) = password {
      model.password =
        Set(UserService::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    model.role_id = NotSet;
    model.organization_id = NotSet;
    model.is_admin = NotSet;
    model.is_enabled = NotSet;

    self.user_ctx.identify().await?;

    Ok(Users::update(model).exec(db::get_connection()).await?)
  }

  pub async fn get_prefs(&self) -> Result<serde_json::Value, SumidaError> {
    let db = db::get_connection();
    let pref = UserPrefs::find_by_id(self.user_ctx.profile.id).one(db).await?;

    Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
  }

  pub async fn update_prefs(&self, data: serde_json::Value) -> Result<(), SumidaError> {
    use entity::user_prefs::ActiveModel;

    let db = db::get_connection();
    let user_id = self.user_ctx.profile.id;
    let pref = UserPrefs::find_by_id(user_id).one(db).await?;
    let mut am = ActiveModel {
      user_id: Set(user_id),
      data: Set(data),
    };

    if pref.is_some() {
      am.user_id = Unchanged(user_id);
    }

    am.save(db).await?;

    Ok(())
  }
}

impl ApiService<Ap, Users> for MeService {
  fn select(&self, query: Select<Users>) -> Select<Users> {
    self.user_service.select(query)
  }

  fn ap(&self) -> Ap {
    self.user_service.ap()
  }
}
