use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Router, routing};
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::{StreamExt, iter};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::{SumidaState, admin_guard};

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

  async fn components(State(state): State<S>) -> Result<DashboardComponents, SumidaError> {
    Ok(MonitoringService::new(state).components())
  }

  async fn system_info(State(state): State<S>) -> Result<SystemInfo, SumidaError> {
    Ok(MonitoringService::new(state).system_info())
  }

  async fn snapshot(State(state): State<S>) -> Result<MonitoringResponse, SumidaError> {
    MonitoringService::new(state).snapshot().await
  }

  async fn stream(
    State(state): State<S>,
  ) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, SumidaError> {
    let service = Arc::new(MonitoringService::new(state));

    // Components Stream
    let components = service.components();
    let components_event = Event::default()
      .event("dashboard.components")
      .json_data(&components)
      .ok()
      .map(|event| Ok::<_, Infallible>(event));

    // System Info Stream
    let system_info = service.system_info();
    let system_info_event = Event::default()
      .event("dashboard.system_info")
      .json_data(&system_info)
      .ok()
      .map(|event| Ok::<_, Infallible>(event));

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
          .map(|event| Ok::<_, Infallible>(event)),
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
}

impl<S> ApiController<S> for DashboardMonitoringController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new()
      .route("/", routing::get(Self::snapshot))
      .route("/components", routing::get(Self::components))
      .route("/system-info", routing::get(Self::system_info))
      .route("/snapshot", routing::get(Self::snapshot))
      .route("/stream", routing::get(Self::stream))
      .route_layer(admin_guard!(state))
  }
}
