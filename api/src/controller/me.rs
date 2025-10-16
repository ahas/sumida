use axum::extract::State;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde_json::json;

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::ap::{self, ApForUser};
use crate::{SumidaState, user_guard};

#[derive(Clone, Debug, Default)]
pub struct MeController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> MeController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn update_me(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<UpdateMe>,
  ) -> Result<MeUpdated, SumidaError> {
    MeService::new(state, session).update_me(body).await?;
    Ok(MeUpdated)
  }

  async fn get_me(session: UserSession, State(state): State<S>) -> Result<Me, SumidaError> {
    MeService::new(state, session).get_session().await?.ok_or(SumidaError::InvalidAccount)
  }

  async fn get_prefs(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    Ok(Json(MeService::new(state, session).get_prefs().await?))
  }

  async fn update_prefs(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<serde_json::Value>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    MeService::new(state, session).update_prefs(body).await?;
    Ok(Json(json!({})))
  }

  async fn get_access_policies(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<Vec<ApForUser>>, SumidaError> {
    let access_policies = ap::for_users().who(&session).find_all(state.db()).await?;
    Ok(Json(access_policies))
  }
}

impl<S> ApiController<S> for MeController<S>
where
  S: SumidaState,
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
