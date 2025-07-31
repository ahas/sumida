use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::boards::*;
use sea_orm::sea_query::{Alias, Expr};
use sea_orm::{Condition, DeriveIntoActiveModel, EntityTrait};
use sea_orm::{QueryTrait, prelude::*};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Deserialize)]
pub struct BoardFilter {
  pub search: Option<String>,
  pub name: Option<String>,
}

impl BoardFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let alias = Alias::new("b");
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Expr::col((alias.clone(), Column::Name)), v))
            .add(wrap_ilike(Expr::col((alias.clone(), Column::Description)), v)),
        )
      })
      .apply_if(self.name.as_ref(), |q, v| q.filter(Expr::col(Column::Name).eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardListItem {
  pub id: i32,
  pub name: String,
  pub category_id: Option<i32>,
  pub description: Option<String>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoard {
  pub id: i32,
  pub name: String,
  pub description: Option<String>,
  pub category_id: Option<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoard {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoard {
  pub name: String,
  pub description: Optional<String>,
  pub category_id: Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCreated {
  pub id: i32,
}

impl IntoResponse for BoardCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoard {
  pub name: Optional<String>,
  pub description: Optional<String>,
  pub category_id: Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardUpdated;

impl IntoResponse for BoardUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardDeleted;

impl IntoResponse for BoardDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
