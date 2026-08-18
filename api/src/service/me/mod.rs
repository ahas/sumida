pub mod dto;

use dto::*;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::{NotSet, Set, Unchanged};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Select};
use sea_orm_crud::Optional;
use serde_json::json;

use crate::SumidaState;
use crate::service::user::UserService;
use crate::util::user::hash_password;

pub struct MeService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> MeService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// `/me` 는 언제나 세션 본인의 행이므로 행 스코프를 붙이지 않는다. 열·조인은
  /// `/users` 와 같은 것을 써야 응답 모양이 갈라지지 않으므로 `UserService` 의 것을 그대로 쓴다.
  fn default_query_with(query: Select<Users>) -> Select<Users> {
    UserService::<S>::default_query_with(query)
  }

  pub async fn get_session(&self) -> Result<Option<Me>, SumidaError> {
    if !self.session.profile.is_authorized {
      return Err(SumidaError::NoCredentials);
    }

    let Some(mut me_session) = Self::default_query_with(Users::find_by_id(self.session.profile.id))
      .into_model::<Me>()
      .one(self.state.db())
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
      model.password = Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    model.role_id = NotSet;
    model.organization_id = NotSet;
    model.is_admin = NotSet;
    model.is_enabled = NotSet;

    let db = self.state.db();

    self.session.identify(db).await?;

    Ok(Users::update(model).exec(db).await?)
  }

  pub async fn get_prefs(&self) -> Result<serde_json::Value, SumidaError> {
    let db = self.state.db();
    let pref = UserPrefs::find_by_id(self.session.profile.id).one(db).await?;

    Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
  }

  pub async fn update_prefs(&self, data: serde_json::Value) -> Result<(), SumidaError> {
    use entity::user_prefs::ActiveModel;

    let db = self.state.db();
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

