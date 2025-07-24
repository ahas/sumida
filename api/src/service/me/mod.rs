pub mod dto;

use core::db;
use core::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{
  ActiveModelTrait,
  ActiveValue::{NotSet, Set, Unchanged},
  EntityTrait, IntoActiveModel,
};
use sea_orm_crud::Optional;
use serde_json::json;

use dto::*;

use crate::service::user::UserService;

pub struct MeService;

impl MeService {
  pub async fn get_session(user_id: i32) -> Result<Option<MeSession>, SumidaError> {
    Ok(
      UserService::select(Users::find_by_id(user_id))
        .into_model::<MeSession>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn update_profile(
    user_ctx: &mut UserContext,
    data: UpdateMeProfile,
  ) -> Result<entity::users::Model, SumidaError> {
    let username = &user_ctx.profile.username;
    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.username = Set(username.clone());

    if let Some(ref password) = password {
      model.password =
        Set(UserService::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    model.role_id = NotSet;
    model.organization_id = NotSet;
    model.is_admin = NotSet;
    model.is_enabled = NotSet;

    user_ctx.identify().await?;

    Ok(Users::update(model).exec(db::get_connection()).await?)
  }

  pub async fn get_prefs(user_id: i32) -> Result<serde_json::Value, SumidaError> {
    let db_conn = db::get_connection();
    let pref = UserPrefs::find_by_id(user_id).one(db_conn).await?;

    Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
  }

  pub async fn update_prefs(user_id: i32, data: serde_json::Value) -> Result<(), SumidaError> {
    use entity::user_prefs::ActiveModel;

    let db = db::get_connection();
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
