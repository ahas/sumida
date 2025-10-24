use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_topics::*;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, EntityTrait, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardTopicFilter {
  pub search:   Option<String>,
  pub name:     Option<String>,
  pub board_id: Option<i32>,
}

impl BoardTopicFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(self.board_id, |q, v| q.filter(Column::BoardId.eq(v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(Column::Name.eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicListItem {
  pub id:          i32,
  pub board_id:    i32,
  pub name:        String,
  pub description: Option<String>,
  pub order:       i32,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardTopic {
  pub id:          i32,
  pub board_id:    i32,
  pub name:        String,
  pub description: Option<String>,
  pub order:       i32,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardTopic {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardTopic {
  pub id:          Optional<i32>,
  pub name:        String,
  pub description: Optional<String>,
  pub order:       Optional<i32>,
  pub board_id:    i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicCreated {
  pub id: i32,
}

impl IntoResponse for BoardTopicCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardTopic {
  pub name:        Optional<String>,
  #[serde(default)]
  pub description: Nullable<String>,
  pub order:       Optional<i32>,
  pub board_id:    Optional<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicUpdated;

impl IntoResponse for BoardTopicUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTopicDeleted;

impl IntoResponse for BoardTopicDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
