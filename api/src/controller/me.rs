use core::db;
use core::session::UserContext;

use axum::{Json, Router, extract::State, routing};
use error::SumidaError;
use serde_json::json;

use crate::service::prelude::*;
use crate::util::ap::UserAp;
use crate::{controller::ApiController, user_guard};

pub struct MeController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> MeController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn get_session(user_ctx: UserContext, State(_state): State<S>) -> Result<MeSession, SumidaError> {
    MeService::get_session(user_ctx.profile.id).await?.ok_or(SumidaError::InvalidAccount)
  }

  async fn get_profile(user_ctx: UserContext, State(_state): State<S>) -> Result<MeProfile, SumidaError> {
    Ok(user_ctx.profile.into())
  }

  async fn update_profile(
    mut user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<UpdateMeProfile>,
  ) -> Result<MeProfileUpdated, SumidaError> {
    MeService::update_profile(&mut user_ctx, body).await?;
    Ok(MeProfileUpdated)
  }

  async fn get_prefs(
    user_ctx: UserContext,
    State(_state): State<S>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    let user_id = user_ctx.profile.id;
    let ret = MeService::get_prefs(user_id).await?;
    Ok(Json(ret))
  }

  async fn update_prefs(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<serde_json::Value>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    let user_id = user_ctx.profile.id;
    MeService::update_prefs(user_id, body).await?;
    Ok(Json(json!({})))
  }

  async fn get_access_policies(
    user_ctx: UserContext,
    State(_state): State<S>,
  ) -> Result<Json<Vec<UserAp>>, SumidaError> {
    let access_policies = UserAp::query().owner(&user_ctx).find_all(db::get_connection()).await?;
    Ok(Json(access_policies))
  }
}

impl<S> ApiController<S> for MeController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new()
      .route("/session", routing::get(Self::get_session))
      .route("/profile", routing::get(Self::get_profile))
      .route("/profile", routing::patch(Self::update_profile))
      .route("/prefs", routing::get(Self::get_prefs))
      .route("/prefs", routing::patch(Self::update_prefs))
      .route("/access-policies", routing::get(Self::get_access_policies))
      .route_layer(user_guard!(state))
  }
}
