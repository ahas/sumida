use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Router, routing};
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::{StreamExt, iter};

use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::{MONITORING_TAG, Refused};
use crate::service::prelude::*;
use crate::{SumidaState, admin_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/dashboard/monitoring";

const COMPONENTS: &str = "/components";
const SYSTEM_INFO: &str = "/system-info";
const SNAPSHOT: &str = "/snapshot";
const STREAM: &str = "/stream";

/// The `event:` names the stream carries, read off the emitter rather than off the handler names.
/// A subscriber that does not know them has nowhere to hang a listener.
#[cfg(feature = "openapi")]
const STREAM_EVENTS: &str = "Three `event:` names ride here. `dashboard.components` and \
`dashboard.system_info` arrive once each, first, and describe the host rather than its load. \
`dashboard.snapshot` follows every second and carries one `MonitoringSnapshot` — the same body \
`GET /dashboard/monitoring/snapshot` answers with, minus the two that came first. A snapshot the \
server fails to collect is logged and skipped rather than sent as an error, so a gap in the series \
is not the end of the stream. Every fifteen seconds a keep-alive comment reading `keep-alive` goes \
by, which carries no event name and must be ignored. The server never closes the stream; the \
subscriber does.";

#[derive(Clone, Debug, Default)]
pub struct DashboardMonitoringController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> DashboardMonitoringController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// Everything the monitoring screen needs to draw itself once: what it may show, what it is running
/// on, and how that host is doing right now.
///
/// This describes the **host process this deployment runs in**, not a row anybody owns, which is why
/// it takes the platform administrator gate rather than a per-resource one.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = MONITORING_TAG,
  operation_id = "dashboard_snapshot",
  responses(
    (status = 200, description = "The panels, the host, and one reading", body = MonitoringResponse),
    Refused,
  ),
))]
async fn snapshot<S: SumidaState>(State(state): State<S>) -> Result<MonitoringResponse, SumidaError> {
  MonitoringService::new(state).snapshot().await
}

/// The same answer as `GET /dashboard/monitoring`, at the path a client that only wants a reading
/// tends to reach for. It is the same body, panels and host description included — not the bare
/// `MonitoringSnapshot` the stream sends.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = SNAPSHOT,
  context_path = MOUNT,
  tag = MONITORING_TAG,
  operation_id = "dashboard_snapshot_at_path",
  responses(
    (status = 200, description = "The panels, the host, and one reading", body = MonitoringResponse),
    Refused,
  ),
))]
async fn snapshot_at_path<S: SumidaState>(
  state: State<S>,
) -> Result<MonitoringResponse, SumidaError> {
  snapshot(state).await
}

/// The panels this build's monitoring screen can show — a key, a title and a line of description
/// each. It is a menu, not a reading.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = COMPONENTS,
  context_path = MOUNT,
  tag = MONITORING_TAG,
  operation_id = "dashboard_components",
  responses(
    (status = 200, description = "The panels this build offers", body = DashboardComponents),
    Refused,
  ),
))]
async fn components<S: SumidaState>(
  State(state): State<S>,
) -> Result<DashboardComponents, SumidaError> {
  Ok(MonitoringService::new(state).components())
}

/// The host: its name, kernel and operating system, how many processors it has, and how much swap
/// there is. These barely move, so a client reads them once rather than polling them.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = SYSTEM_INFO,
  context_path = MOUNT,
  tag = MONITORING_TAG,
  operation_id = "dashboard_system_info",
  responses(
    (status = 200, description = "What the server is running on", body = SystemInfo),
    Refused,
  ),
))]
async fn system_info<S: SumidaState>(State(state): State<S>) -> Result<SystemInfo, SumidaError> {
  Ok(MonitoringService::new(state).system_info())
}

/// Watch the host live (SSE).
///
/// Everything the one-shot read answers with arrives here too, split across named events, and then
/// the load reading repeats on its own.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = STREAM,
  context_path = MOUNT,
  tag = MONITORING_TAG,
  operation_id = "dashboard_stream",
  responses(
    (status = 200, description = STREAM_EVENTS, content_type = "text/event-stream"),
    Refused,
  ),
))]
async fn stream<S: SumidaState>(
  State(state): State<S>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, SumidaError> {
  let service = Arc::new(MonitoringService::new(state));

  // Components Stream
  let components = service.components();
  let components_event = Event::default()
    .event("dashboard.components")
    .json_data(&components)
    .ok()
    .map(Ok::<_, Infallible>);

  // System Info Stream
  let system_info = service.system_info();
  let system_info_event = Event::default()
    .event("dashboard.system_info")
    .json_data(&system_info)
    .ok()
    .map(Ok::<_, Infallible>);

  // Snapshot Stream
  let mut interval = tokio::time::interval(Duration::from_secs(1));
  interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
  let snapshot_stream = IntervalStream::new(interval)
    .then(move |_| {
      let service = Arc::clone(&service);

      async move { service.snapshot_only().await }
    })
    .filter_map(|result| match result {
      Ok(snapshot) => Event::default()
        .event("dashboard.snapshot")
        .json_data(&snapshot)
        .ok()
        .map(Ok::<_, Infallible>),
      Err(err) => {
        tracing::error!(error = %err, "failed to collect dashboard snapshot");
        None
      }
    });

  // Merge Initial Events
  let mut initial_events = Vec::new();

  if let Some(event) = components_event {
    initial_events.push(event);
  }

  if let Some(event) = system_info_event {
    initial_events.push(event);
  }

  let stream = iter(initial_events).chain(snapshot_stream);

  Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("keep-alive")))
}

impl<S> ApiController<S> for DashboardMonitoringController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new()
      .route("/", routing::get(snapshot::<S>))
      .route(COMPONENTS, routing::get(components::<S>))
      .route(SYSTEM_INFO, routing::get(system_info::<S>))
      .route(SNAPSHOT, routing::get(snapshot_at_path::<S>))
      .route(STREAM, routing::get(stream::<S>))
      .route_layer(admin_guard!(state))
  }
}
