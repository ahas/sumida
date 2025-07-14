use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::DeriveIntoActiveModel;
use sea_orm::EntityTrait;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm_crud::Optional;
use serde::{Deserialize, Serialize};
use serde_json::json;

use entity::bs::board_categories::*;

use crate::util::crud::wrap_ilike;

#[derive(Deserialize)]
pub struct BoardCategoryFilter {
  pub search: Option<String>,
  pub name: Option<String>,
}

impl BoardCategoryFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let alias = Alias::new("bc");
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(wrap_ilike(Expr::col((alias.clone(), Column::Name)), v))
      })
      .apply_if(self.name.as_ref(), |q, v| q.filter(Expr::col(Column::Name).eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardCategoryListItem {
  pub id: i32,
  pub name: String,
  pub description: Option<String>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardCategory {
  pub id: i32,
  pub name: String,
  pub description: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardCategory {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardCategory {
  pub id: Optional<i32>,
  pub name: String,
  pub description: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCategoryCreated {
  pub id: i32,
}

impl IntoResponse for BoardCategoryCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardCategory {
  pub name: Optional<String>,
  pub description: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCategoryUpdated;

impl IntoResponse for BoardCategoryUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCategoryDeleted;

impl IntoResponse for BoardCategoryDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
