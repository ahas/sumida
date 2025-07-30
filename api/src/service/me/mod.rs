pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::db;
use infra::session::UserSession;
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
use crate::util::ap::ApCondition;

pub struct MeService {
  session: UserSession,
  user_service: UserService,
}

impl MeService {
  pub fn new(session: UserSession) -> Self {
    Self {
      user_service: UserService::new(session.clone()),
      session,
    }
  }

  pub async fn get_session(&self) -> Result<Option<Me>, SumidaError> {
    if !self.session.profile.is_authorized {
      return Err(SumidaError::NoCredentials);
    }

    let Some(mut me_session) = self
      .select(Users::find_by_id(self.session.profile.id))
      .into_model::<Me>()
      .one(db::get_connection())
      .await?
    else {
      return Ok(None);
    };

    me_session.first_seen_at = self.session.profile.first_seen_at;
    me_session.last_seen_at = self.session.profile.last_seen_at;

    Ok(Some(me_session))
  }

  pub async fn update_me(&mut self, data: UpdateMe) -> Result<entity::users::Model, SumidaError> {
    let username = &self.session.profile.username;
    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.id = Unchanged(self.session.profile.id);
    model.username = Set(username.clone());

    if let Some(ref password) = password {
      model.password =
        Set(UserService::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    model.role_id = NotSet;
    model.organization_id = NotSet;
    model.is_admin = NotSet;
    model.is_enabled = NotSet;

    self.session.identify().await?;

    Ok(Users::update(model).exec(db::get_connection()).await?)
  }

  pub async fn get_prefs(&self) -> Result<serde_json::Value, SumidaError> {
    let db = db::get_connection();
    let pref = UserPrefs::find_by_id(self.session.profile.id).one(db).await?;

    Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
  }

  pub async fn update_prefs(&self, data: serde_json::Value) -> Result<(), SumidaError> {
    use entity::user_prefs::ActiveModel;

    let db = db::get_connection();
    let user_id = self.session.profile.id;
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

impl ApiService<ApCondition, Users> for MeService {
  fn select(&self, query: Select<Users>) -> Select<Users> {
    self.user_service.select(query)
  }

  fn ap(&self) -> ApCondition {
    self.user_service.ap()
  }
}
