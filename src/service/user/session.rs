use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::Utc;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tower_sessions::{Expiry, Session};

use crate::core::db;
use crate::core::error::SumidaError;

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

  pub async fn verify(&self) -> Result<(), SumidaError> {
    let mut user_data = self.user_data.clone();

    if !user_data.is_authorized {
      return Err(if user_data.username.is_empty() {
        SumidaError::NoCredentials
      } else {
        SumidaError::NotAuthorized(user_data.username)
      });
    }

    let db_conn = db::get_connection();
    let user = crate::entity::users::Entity::find_by_id(&user_data.username)
      .one(db_conn)
      .await?
      .ok_or(SumidaError::NoUserAccount(user_data.username))?;
    let organization = crate::entity::organizations::Entity::find_by_id(user.organization)
      .one(db_conn)
      .await?
      .ok_or(SumidaError::NoUserOrganization(user.organization))?;
    let role = crate::entity::roles::Entity::find_by_id(user.role)
      .one(db_conn)
      .await?
      .ok_or(SumidaError::NoUserRole(user.role))?;

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

  pub async fn sign_in(
    &self,
    user: &crate::entity::users::Model,
    expiry: Option<Expiry>,
  ) -> Result<(), SumidaError> {
    let db_conn = db::get_connection();
    let organization =
      user.get_organization(db_conn).await?.ok_or(SumidaError::NoUserOrganization(user.organization))?;
    let organization = OrganizationData {
      id: organization.id,
      name: organization.name.clone(),
    };
    let role = user.get_role(db_conn).await?.ok_or(SumidaError::NoUserRole(user.role))?;
    let role = RoleData {
      id: role.id,
      name: role.name.clone(),
    };

    Self::update(
      &self.session,
      &UserData {
        is_authorized: true,
        username: user.username.clone(),
        full_name: user.full_name.clone().unwrap_or("".to_owned()),
        organization,
        role,
        is_admin: user.is_admin,
        expiry,
        ..Default::default()
      },
    )
    .await?;

    Ok(())
  }
}

impl<S> FromRequestParts<S> for UserSession
where
  S: Send + Sync,
{
  type Rejection = SumidaError;

  async fn from_request_parts(req: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let session = Session::from_request_parts(req, state).await?;

    let mut user_data: UserData = session.get(Self::USER_DATA_KEY).await.unwrap().unwrap_or_default();

    user_data.last_seen_at = Utc::now().into();
    Self::update(&session, &user_data).await?;

    Ok(Self { session, user_data })
  }
}
