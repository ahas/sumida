use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Json, Router, routing};
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

  async fn components(State(state): State<S>) -> Result<Json<DashboardComponents>, SumidaError> {
    let components = MonitoringService::new(state).components();
    Ok(Json(components))
  }

  async fn snapshot(State(state): State<S>) -> Result<Json<MonitoringResponse>, SumidaError> {
    let response = MonitoringService::new(state).snapshot().await?;
    Ok(Json(response))
  }

  async fn stream(
    State(state): State<S>,
  ) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, SumidaError> {
    let service = MonitoringService::new(state);
    let components = service.components();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let snapshot_stream = IntervalStream::new(interval)
      .then(move |_| {
        let service = service.clone();

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

    let components_event = Event::default()
      .event("dashboard.components")
      .json_data(&components)
      .ok()
      .map(|event| Ok::<_, Infallible>(event));

    let stream = match components_event {
      Some(event) => iter(vec![event]).chain(snapshot_stream),
      None => iter(Vec::new()).chain(snapshot_stream),
    };

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
      .route("/stream", routing::get(Self::stream))
      .route_layer(admin_guard!(state))
  }
}
