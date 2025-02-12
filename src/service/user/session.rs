use crate::core::db;
use crate::core::error::AppError;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tower_sessions::{Expiry, Session};

use sea_orm::prelude::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserData {
  pub username: String,
  pub full_name: String,
  pub organization: OrganizationData,
  pub role: RoleData,
  pub ability: JsonValue,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub first_seen_at: DateTimeWithTimeZone,
  pub last_seen_at: DateTimeWithTimeZone,
  pub expiry: Option<Expiry>,
}

impl Default for UserData {
  fn default() -> Self {
    Self {
      username: "".to_owned(),
      full_name: "".to_owned(),
      organization: OrganizationData::default(),
      role: RoleData::default(),
      ability: json!({}),
      is_admin: false,
      is_authorized: false,
      first_seen_at: Utc::now().into(),
      last_seen_at: Utc::now().into(),
      expiry: None,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrganizationData {
  pub id: i32,
  pub name: String,
}

impl Default for OrganizationData {
  fn default() -> Self {
    Self {
      id: 0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoleData {
  pub id: i32,
  pub name: String,
}

impl Default for RoleData {
  fn default() -> Self {
    Self {
      id: 0,
      name: "".to_owned(),
    }
  }
}

#[derive(Clone, Debug)]
pub struct UserSession {
  pub session: Session,
  pub user_data: UserData,
}

impl UserSession {
  pub const USER_DATA_KEY: &'static str = "user.data";

  pub async fn update(session: &Session, user_data: &UserData) -> Result<(), anyhow::Error> {
    session.insert(Self::USER_DATA_KEY, user_data.clone()).await?;
    session.set_expiry(user_data.expiry);
    session.save().await?;

    Ok(())
  }

  pub async fn verify(&self) -> Result<(), AppError> {
    let mut user_data = self.user_data.clone();

    if !user_data.is_authorized {
      return Err(AppError::NotAuthorized(user_data.username));
    }

    let db_conn = db::get_connection();
    let user = crate::entity::users::Entity::find_by_id(&user_data.username)
      .one(db_conn)
      .await?
      .ok_or_else(|| AppError::NoUserAccount(user_data.username))?;
    let organization = crate::entity::organizations::Entity::find_by_id(user.organization)
      .one(db_conn)
      .await?
      .ok_or_else(|| AppError::NoUserOrganization(user.organization))?;
    let role = crate::entity::roles::Entity::find_by_id(user.role)
      .one(db_conn)
      .await?
      .ok_or_else(|| AppError::NoUserRole(user.role))?;

    user_data.username = user.username.clone();
    user_data.organization = OrganizationData {
      id: organization.id,
      name: organization.name.clone(),
    };
    user_data.role = RoleData {
      id: role.id,
      name: role.name.clone(),
    };
    user_data.is_admin = user.is_admin;
    user_data.last_seen_at = Utc::now().into();
    user_data.ability = role.ability;

    Self::update(&self.session, &user_data).await?;

    Ok(())
  }
}

impl<S> FromRequestParts<S> for UserSession
where
  S: Send + Sync,
{
  type Rejection = AppError;

  async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let session = Session::from_request_parts(req, state).await?;

    let mut user_data: UserData = session.get(Self::USER_DATA_KEY).await.unwrap().unwrap_or_default();

    user_data.last_seen_at = Utc::now().into();
    Self::update(&session, &user_data).await?;

    Ok(Self { session, user_data })
  }
}
