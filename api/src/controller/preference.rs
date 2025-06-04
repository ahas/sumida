use axum::extract::State;
use axum::{routing, Json, Router};
use serde_json::json;

use crate::core::error::SumidaError;
use crate::service::preference as PreferenceService;
use crate::service::prelude::*;
use crate::user_guard;

async fn get<S>(
  user_ctx: UserContext,
  State(_state): State<S>,
) -> Result<Json<serde_json::Value>, SumidaError> {
  let username = user_ctx.profile.username.clone();
  let ret = PreferenceService::get(username).await?;
  Ok(Json(ret))
}

async fn update<S>(
  user_ctx: UserContext,
  State(_state): State<S>,
  Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, SumidaError> {
  let username = user_ctx.profile.username.clone();
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
