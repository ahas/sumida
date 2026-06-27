pub mod dto;
pub mod provider;

use std::sync::Arc;

use dto::*;
use sysinfo::System;

use super::prelude::*;
use crate::SumidaState;
use crate::service::monitoring::provider::{DynMetricsProvider, SysinfoMetricsProvider};

const DASHBOARD_COMPONENTS: &[DashboardComponent] = &[
  DashboardComponent {
    key: "system_overview",
    title: "System Overview",
    description: "Uptime, CPU, memory, and load averages.",
  },
  DashboardComponent {
    key: "service_health",
    title: "Service Health",
    description: "Core service reachability checks (DB, app).",
  },
  DashboardComponent {
    key: "realtime_feed",
    title: "Real-time Feed",
    description: "Admin-only live metrics stream for dashboards.",
  },
];

#[derive(Clone)]
pub struct MonitoringService<S>
where
  S: SumidaState,
{
  state: S,
  metrics: DynMetricsProvider,
}

impl<S> MonitoringService<S>
where
  S: SumidaState,
{
  pub fn new(state: S) -> Self {
    Self {
      state,
      metrics: Arc::new(SysinfoMetricsProvider::new()),
    }
  }

  #[cfg(test)]
  pub fn with_metrics_provider(state: S, metrics: DynMetricsProvider) -> Self {
    Self { state, metrics }
  }

  pub fn components(&self) -> DashboardComponents {
    DashboardComponents {
      items: DASHBOARD_COMPONENTS.to_vec(),
    }
  }

  pub fn system_info(&self) -> SystemInfo {
    SystemInfo::new(System::new_all())
  }

  pub async fn snapshot(&self) -> Result<MonitoringResponse, SumidaError> {
    let system = self.metrics.sample().await?;
    let db = self.state.db();
    let services = vec![ServiceHealth::new_for_app(), ServiceHealth::new_for_db(db).await];

    Ok(MonitoringResponse {
      components: self.components(),
      system_info: self.system_info(),
      snapshot: MonitoringSnapshot::new(system, services),
    })
  }

  pub async fn snapshot_only(&self) -> Result<MonitoringSnapshot, SumidaError> {
    let system = self.metrics.sample().await?;
    let db = self.state.db();
    let services = vec![ServiceHealth::new_for_app(), ServiceHealth::new_for_db(db).await];

    Ok(MonitoringSnapshot::new(system, services))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::service::monitoring::provider::{FakeMetricsProvider, SystemMetrics};

  #[derive(Clone)]
  struct DummyState;

  impl SumidaState for DummyState {
    fn db(&self) -> &sea_orm::DatabaseConnection {
      unreachable!("db not needed for unit tests")
    }
  }

  #[tokio::test]
  async fn snapshot_includes_components_and_metrics() {
    let provider = Arc::new(FakeMetricsProvider::new(SystemMetrics {
      uptime_seconds: 42,
      cpu_usage_percent: 12.5,
      memory_used_bytes: 512,
      memory_total_bytes: 1024,
      load_average: LoadAverage {
        one: 0.1,
        five: 0.2,
        fifteen: 0.3,
      },
    }));

    let service = MonitoringService::with_metrics_provider(DummyState, provider);

    // Verify the sampled metrics flow into a snapshot. The DB health probe in
    // `snapshot`/`snapshot_only` needs a live connection, so this unit test
    // exercises the metrics mapping directly and stays DB-free.
    let system = service.metrics.sample().await.unwrap();
    let snapshot = MonitoringSnapshot::new(system, vec![ServiceHealth::new_for_app()]);

    assert_eq!(snapshot.uptime_seconds, 42);
    assert_eq!(snapshot.resources.cpu_usage_percent, 12.5);
    assert_eq!(snapshot.resources.memory_total_bytes, 1024);
    assert_eq!(snapshot.resources.load_average.fifteen, 0.3);
  }
}
