use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_posts::*;
use sea_orm::Condition;
use sea_orm::DeriveIntoActiveModel;
use sea_orm::EntityTrait;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Deserialize)]
pub struct BoardPostFilter {
  pub search: Option<String>,
  pub title: Option<String>,
}

impl BoardPostFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let alias = Alias::new("bp");
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Expr::col((alias.clone(), Column::Title)), v))
            .add(wrap_ilike(Expr::col((alias.clone(), Column::Content)), v)),
        )
      })
      .apply_if(self.title.as_ref(), |q, v| q.filter(Expr::col(Column::Title).eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostListItem {
  pub id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub title: String,
  pub content: Option<String>,
  #[sea_orm(skip)]
  pub view_count: i32,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPost {
  pub id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub title: String,
  pub content: Option<String>,
  #[sea_orm(skip)]
  pub view_count: i32,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPost {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPost {
  pub id: Optional<i32>,
  pub parent_id: Nullable<i32>,
  pub user_id: Nullable<i32>,
  pub title: String,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPost {
  pub title: Optional<String>,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostUpdated;

impl IntoResponse for BoardPostUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostDeleted;

impl IntoResponse for BoardPostDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}
