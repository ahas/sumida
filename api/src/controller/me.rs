use infra::db;
use infra::session::UserContext;

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

  async fn update_me(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<UpdateMe>,
  ) -> Result<MeUpdated, SumidaError> {
    MeService::new(user_ctx).update_me(body).await?;
    Ok(MeUpdated)
  }

  async fn get_me(user_ctx: UserContext, State(_state): State<S>) -> Result<Me, SumidaError> {
    MeService::new(user_ctx).get_session().await?.ok_or(SumidaError::InvalidAccount)
  }

  async fn get_prefs(
    user_ctx: UserContext,
    State(_state): State<S>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    Ok(Json(MeService::new(user_ctx).get_prefs().await?))
  }

  async fn update_prefs(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<serde_json::Value>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    MeService::new(user_ctx).update_prefs(body).await?;
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
      .route("/", routing::get(Self::get_me))
      .route("/", routing::patch(Self::update_me))
      .route("/prefs", routing::get(Self::get_prefs))
      .route("/prefs", routing::patch(Self::update_prefs))
      .route("/access-policies", routing::get(Self::get_access_policies))
      .route_layer(user_guard!(state))
  }
}
