use axum::extract::State;
use axum::{Json, Router, routing};
use serde_json::json;

use crate::service::prelude::*;
use crate::user_guard;

use super::ApiController;

pub struct PreferenceController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> PreferenceController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn get(
    user_ctx: UserContext,
    State(_state): State<S>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    let user_id = user_ctx.profile.id;
    let ret = PreferenceService::get(user_id).await?;
    Ok(Json(ret))
  }

  async fn update(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<serde_json::Value>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    let user_id = user_ctx.profile.id;
    PreferenceService::update(user_id, body).await?;
    Ok(Json(json!({})))
  }
}

impl<S> ApiController<S> for PreferenceController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::<S>::new()
      .route("/", routing::get(Self::get))
      .route("/", routing::patch(Self::update))
      .route_layer(user_guard!(state))
  }
}
