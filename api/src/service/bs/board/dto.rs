use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::boards::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, Expr};
use sea_orm::{Condition, DeriveIntoActiveModel, EntityTrait, FromQueryResult, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[sea_orm(entity = "entity::bs::board_categories::Entity")]
pub struct BoardBoardCategory {
  #[sea_orm(from_alias = "bc_id")]
  pub id: i32,
  #[sea_orm(from_alias = "bc_name")]
  pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardListItem {
  pub id: i32,
  #[sea_orm(nested, alias = "bc")]
  pub category: Option<BoardBoardCategory>,
  pub name: String,
  pub description: Option<String>,
  pub can_upload_file: bool,
  pub is_logged: bool,
  pub is_enabled: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoard {
  pub id: i32,
  pub category_id: Option<i32>,
  pub name: String,
  pub description: Option<String>,
  pub can_upload_file: bool,
  pub max_file_size: i64,
  pub max_file_count: i32,
  pub allowed_file_types: Option<JsonValue>,
  pub reaction_visibility: BoardPostRecordVisibility,
  pub viewer_visibility: BoardPostRecordVisibility,
  pub is_logged: bool,
  pub is_enabled: bool,
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
  pub id: Optional<i32>,
  #[serde(default)]
  pub category_id: Nullable<i32>,
  pub name: String,
  #[serde(default)]
  pub description: Nullable<String>,
  pub can_upload_file: Optional<bool>,
  pub max_file_size: Optional<i64>,
  pub max_file_count: Optional<i32>,
  pub allowed_file_types: Optional<JsonValue>,
  pub reaction_visibility: Optional<BoardPostRecordVisibility>,
  pub viewer_visibility: Optional<BoardPostRecordVisibility>,
  pub is_logged: Optional<bool>,
  pub is_enabled: Optional<bool>,
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
  #[serde(default)]
  pub category_id: Nullable<i32>,
  pub name: Optional<String>,
  pub description: Optional<String>,
  pub can_upload_file: Optional<bool>,
  pub max_file_size: Optional<i64>,
  pub max_file_count: Optional<i32>,
  pub allowed_file_types: Optional<JsonValue>,
  pub reaction_visibility: Optional<BoardPostRecordVisibility>,
  pub viewer_visibility: Optional<BoardPostRecordVisibility>,
  pub is_logged: Optional<bool>,
  pub is_enabled: Optional<bool>,
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
