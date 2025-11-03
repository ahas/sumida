use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::users::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, CommonTableExpression, Query, UnionType, WithClause};
use sea_orm::{Condition, FromQueryResult, JoinType, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
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
            .add(wrap_ilike(Column::Username.into_expr(), v))
            .add(wrap_ilike(Column::FullName.into_expr(), v)),
        )
      })
      .apply_if(self.username.as_ref(), |q, v| q.filter(wrap_ilike(Column::Username.into_expr(), v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::FullName.into_expr(), v)))
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

impl From<infra::session::UserOrganization> for UserOrganization {
  fn from(value: infra::session::UserOrganization) -> Self {
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

impl From<infra::session::UserRole> for UserRole {
  fn from(value: infra::session::UserRole) -> Self {
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
