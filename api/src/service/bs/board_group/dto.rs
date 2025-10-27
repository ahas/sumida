use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_groups::*;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, EntityTrait, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardGroupFilter {
  pub search:    Option<String>,
  pub name:      Option<String>,
  pub parent_id: Option<i32>,
}

impl BoardGroupFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let query = if let Some(parent_id) = self.parent_id {
      query.filter(Column::ParentId.eq(parent_id))
    } else {
      query.filter(Column::ParentId.is_null())
    };

    query
      .apply_if(self.search.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(Column::Name.eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupListItem {
  pub id:          i32,
  pub name:        String,
  pub description: Option<String>,
  pub order:       i32,
  pub parent_id:   Option<i32>,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardGroup {
  pub id:          i32,
  pub parent_id:   Option<i32>,
  pub name:        String,
  pub description: Option<String>,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardGroup {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardGroup {
  pub id:          Optional<i32>,
  pub name:        String,
  pub description: Optional<String>,
  #[serde(default)]
  pub parent_id:   Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupCreated {
  pub id: i32,
}

impl IntoResponse for BoardGroupCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardGroup {
  pub name:        Optional<String>,
  pub description: Optional<String>,
  #[serde(default)]
  pub parent_id:   Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupUpdated;

impl IntoResponse for BoardGroupUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardGroupDeleted;

impl IntoResponse for BoardGroupDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
