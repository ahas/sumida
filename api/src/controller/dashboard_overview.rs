use axum::extract::State;
use axum::{Router, routing};

use crate::controller::ApiController;
use crate::service::dashboard::{AccountActivity, AuthActivity, DashboardService, StorageUsage};
use crate::service::prelude::*;
use crate::{SumidaState, admin_guard};

/// Installation-wide reads for the system dashboard. They sit behind the same guard as the host
/// monitoring beside them: both answer questions about the deployment rather than about a row
/// somebody owns.
#[derive(Clone, Debug, Default)]
pub struct DashboardOverviewController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> DashboardOverviewController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn auth_activity(State(state): State<S>) -> Result<AuthActivity, SumidaError> {
    DashboardService::auth_activity(state.db()).await
  }

  async fn account_activity(State(state): State<S>) -> Result<AccountActivity, SumidaError> {
    DashboardService::account_activity(state.db()).await
  }

  async fn storage_usage(State(state): State<S>) -> Result<StorageUsage, SumidaError> {
    DashboardService::storage_usage(state.db()).await
  }
}

impl<S: SumidaState> ApiController<S> for DashboardOverviewController<S> {
  fn router(self, state: &S) -> Router<S> {
    Router::new()
      .route("/auth", routing::get(Self::auth_activity))
      .route("/accounts", routing::get(Self::account_activity))
      .route("/storage", routing::get(Self::storage_usage))
      .route_layer(admin_guard!(state))
  }
}
