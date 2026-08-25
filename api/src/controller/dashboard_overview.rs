use axum::extract::State;
use axum::{Router, routing};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{OVERVIEW_TAG, Refused};
use crate::service::dashboard::{AccountActivity, AuthActivity, DashboardService, StorageUsage};
use crate::service::prelude::*;
use crate::{SumidaState, admin_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
///
/// The monitoring routes are nested one level deeper under the same word, and axum matches the
/// longer prefix first, so `/dashboard/monitoring` never reaches this controller.
pub const MOUNT: &str = "/dashboard";

const AUTH: &str = "/auth";
const ACCOUNTS: &str = "/accounts";
const STORAGE: &str = "/storage";

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
}

/// Sign-ins over the recent window: how many succeeded and how many were refused each day, and the
/// addresses refused most often.
///
/// `from` and `to` mark the window that was asked for, so a day with no attempt at all is a gap the
/// client can draw rather than a day missing from the series.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = AUTH,
  context_path = MOUNT,
  tag = OVERVIEW_TAG,
  operation_id = "dashboard_overview_auth_activity",
  responses(
    (status = 200, description = "The daily series and the addresses behind the refusals", body = AuthActivity),
    Refused,
  ),
))]
async fn auth_activity<S: SumidaState>(State(state): State<S>) -> Result<AuthActivity, SumidaError> {
  DashboardService::auth_activity(state.db()).await
}

/// What this installation holds: how many accounts, organizations, roles, keys and bans there are,
/// and how sign-ups arrived over the recent window.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ACCOUNTS,
  context_path = MOUNT,
  tag = OVERVIEW_TAG,
  operation_id = "dashboard_overview_account_activity",
  responses(
    (status = 200, description = "The totals and the sign-up series", body = AccountActivity),
    Refused,
  ),
))]
async fn account_activity<S: SumidaState>(
  State(state): State<S>,
) -> Result<AccountActivity, SumidaError> {
  DashboardService::account_activity(state.db()).await
}

/// What the uploads add up to, broken down by media type.
///
/// `unreferenced` counts rows nothing points at any more — what a sweep would be able to remove.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = STORAGE,
  context_path = MOUNT,
  tag = OVERVIEW_TAG,
  operation_id = "dashboard_overview_storage_usage",
  responses(
    (status = 200, description = "Files, bytes, and the media types they fall into", body = StorageUsage),
    Refused,
  ),
))]
async fn storage_usage<S: SumidaState>(State(state): State<S>) -> Result<StorageUsage, SumidaError> {
  DashboardService::storage_usage(state.db()).await
}

impl<S: SumidaState> ApiController<S> for DashboardOverviewController<S> {
  fn router(self, state: &S) -> Router<S> {
    Router::new()
      .route(AUTH, routing::get(auth_activity::<S>))
      .route(ACCOUNTS, routing::get(account_activity::<S>))
      .route(STORAGE, routing::get(storage_usage::<S>))
      .route_layer(admin_guard!(state))
  }
}
