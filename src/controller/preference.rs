use crate::user_guard;

use crate::core::error::AppError;
use crate::service::preference as PreferenceService;
use crate::service::prelude::*;

use axum::extract::State;
use axum::{routing, Json, Router};
use serde_json::json;

async fn get<S>(
  session: UserSession,
  State(_state): State<S>,
) -> Result<Json<serde_json::Value>, AppError> {
  let username = session.user_data.username.clone();
  let ret = PreferenceService::get(username).await?;
  Ok(Json(ret))
}

async fn update<S>(
  session: UserSession,
  State(_state): State<S>,
  Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
  let username = session.user_data.username.clone();
  PreferenceService::update(username, body).await?;
  Ok(Json(json!({})))
}

pub fn new<S>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  Router::<S>::new()
    .route("/", routing::get(get::<S>))
    .route("/", routing::patch(update::<S>))
    .route_layer(user_guard!(state))
}
