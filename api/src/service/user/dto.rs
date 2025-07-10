use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::Condition;
use sea_orm::JoinType;
use sea_orm::QueryTrait;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::CommonTableExpression;
use sea_orm::sea_query::Query;
use sea_orm::sea_query::UnionType;
use sea_orm::sea_query::WithClause;
// use core::session::{UserOrganization, UserRole};
use core::settings::web_push::WEB_PUSH_PRIVATE_KEY_PEM;
use entity::users::*;
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::util::crud::wrap_ilike_col;

#[derive(Deserialize)]
pub struct UserFilter {
  pub search: Option<String>,
  pub username: Option<String>,
  pub name: Option<String>,
  pub organization_id: Option<OneOrMany<i32>>,
  pub role_id: Option<i32>,
  pub recursive: Option<bool>,
}

impl UserFilter {
  pub fn apply<E: EntityTrait>(&self, mut query: Select<E>) -> Select<E> {
    query = query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(wrap_ilike_col(Column::Username, &v))
            .add(wrap_ilike_col(Column::FullName, &v)),
        )
      })
      .apply_if(self.username.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Username, v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::FullName, v)))
      .apply_if(self.role_id, |q, v| q.filter(Column::RoleId.eq(v)));

    if let Some(org_id) = &self.organization_id {
      if self.recursive.unwrap_or_default() {
        let mut base =
          org_id.into_select_statement(entity::organizations::Entity, entity::organizations::Column::Id);

        let recurse = Query::select()
          .column((Alias::new("org_child"), entity::organizations::Column::Id))
          .from_as(entity::organizations::Entity, Alias::new("org_child"))
          .join(
            JoinType::InnerJoin,
            Alias::new("org_tree"),
            Expr::col((Alias::new("org_child"), entity::organizations::Column::ParentId))
              .equals((Alias::new("org_tree"), Alias::new("id"))),
          )
          .to_owned();

        let cte_query = base.union(UnionType::All, recurse).to_owned();
        let mut org_tree_cte = CommonTableExpression::new();
        org_tree_cte.table_name(Alias::new("org_tree")).query(cte_query);

        let mut with = WithClause::new();
        with.recursive(true);
        with.cte(org_tree_cte.clone());

        let mut sub = Query::select().column(Alias::new("id")).from(Alias::new("org_tree")).to_owned();
        sub.with_cte(with);

        query = query.filter(Expr::col(Column::OrganizationId).in_subquery(sub));
      } else {
        query = match org_id {
          OneOrMany::One(v) => query.filter(Column::OrganizationId.eq(*v)),
          OneOrMany::Many(v) => query.filter(Column::OrganizationId.is_in(v.clone())),
        };
      }
    }

    query
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserOrganization {
  #[sea_orm(from_alias = "org_id")]
  pub id: i32,
  #[sea_orm(from_alias = "org_name")]
  pub name: String,
}

impl From<core::session::UserOrganization> for UserOrganization {
  fn from(value: core::session::UserOrganization) -> Self {
    Self {
      id: value.id,
      name: value.name,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserRole {
  #[sea_orm(from_alias = "role_id")]
  pub id: i32,
  #[sea_orm(from_alias = "role_name")]
  pub name: String,
}

impl From<core::session::UserRole> for UserRole {
  fn from(value: core::session::UserRole) -> Self {
    Self {
      id: value.id,
      name: value.name,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserListItem {
  pub id: i32,
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  #[sea_orm(from_alias = "is_blocked")]
  pub is_blocked: bool,
  #[sea_orm(nested, alias = "org")]
  pub organization: UserOrganization,
  #[sea_orm(nested, alias = "role")]
  pub role: UserRole,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetUser {
  pub id: i32,
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  #[sea_orm(from_alias = "is_blocked")]
  pub is_blocked: bool,
  #[sea_orm(nested)]
  pub organization: UserOrganization,
  #[sea_orm(nested)]
  pub role: UserRole,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetUser {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateUser {
  pub id: Optional<i32>,
  pub username: String,
  pub password: String,
  pub full_name: Nullable<String>,
  pub email: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization_id: i32,
  pub role_id: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
  pub id: i32,
}

impl IntoResponse for UserCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUser {
  pub username: Optional<String>,
  pub password: Optional<String>,
  pub email: Nullable<String>,
  pub full_name: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization_id: Optional<i32>,
  pub role_id: Optional<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdated;

impl IntoResponse for UserUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeleted;

impl IntoResponse for UserDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInUser {
  pub username: String,
  pub password: String,
  pub remember_me: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
  pub username: String,
  pub full_name: String,
  pub organization: UserOrganization,
  pub role: UserRole,
  pub ability: JsonValue,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub first_seen_at: DateTimeWithTimeZone,
  pub last_seen_at: DateTimeWithTimeZone,
}

impl From<core::session::UserProfile> for UserProfile {
  fn from(value: core::session::UserProfile) -> Self {
    Self {
      username: value.username.clone(),
      full_name: value.full_name.clone(),
      organization: UserOrganization::from(value.organization.clone()),
      role: UserRole::from(value.role.clone()),
      ability: value.ability.clone(),
      is_admin: value.is_admin,
      is_authorized: value.is_authorized,
      first_seen_at: value.first_seen_at,
      last_seen_at: value.last_seen_at,
    }
  }
}

impl IntoResponse for UserProfile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
  pub username: String,
  pub password: String,
  pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivation {
  pub user_id: i32,
  pub password: String,
  pub uuid: Uuid,
}

#[derive(Debug, Serialize)]
pub struct VapidClaims {
  pub aud: String,
  pub exp: usize,
  pub sub: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetVapid {
  pub token: String,
}

impl GetVapid {
  pub async fn new(audience: &str, expiration: &str, subject: &str) -> Self {
    let expiration = chrono::Utc::now()
      + humantime::parse_duration(expiration).unwrap_or_else(|_| std::time::Duration::from_secs(0));
    let claims = VapidClaims {
      aud: audience.to_string(),
      exp: expiration.timestamp() as usize,
      sub: subject.to_string(),
    };
    let header = jsonwebtoken::Header {
      alg: jsonwebtoken::Algorithm::ES256,
      ..Default::default()
    };
    let token =
      jsonwebtoken::encode(&header, &claims, &WEB_PUSH_PRIVATE_KEY_PEM).expect("jwt encoding failed");

    GetVapid { token }
  }
}

impl IntoResponse for GetVapid {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}
