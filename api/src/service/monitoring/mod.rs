use std::sync::Arc;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::prelude::*;
use crate::SumidaState;
use crate::service::monitoring::provider::{DynMetricsProvider, SysinfoMetricsProvider};

pub mod dto;
pub mod provider;

const DASHBOARD_COMPONENTS: &[dto::DashboardComponent] = &[
  dto::DashboardComponent {
    key: "system_overview",
    title: "System Overview",
    description: "Uptime, CPU, memory, and load averages.",
  },
  dto::DashboardComponent {
    key: "service_health",
    title: "Service Health",
    description: "Core service reachability checks (DB, app).",
  },
  dto::DashboardComponent {
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
  check_database: bool,
}

impl<S> MonitoringService<S>
where
  S: SumidaState,
{
  pub fn new(state: S) -> Self {
    Self {
      state,
      metrics: Arc::new(SysinfoMetricsProvider::new()),
      check_database: true,
    }
  }

  #[cfg(test)]
  pub fn with_metrics_provider(state: S, metrics: DynMetricsProvider) -> Self {
    Self {
      state,
      metrics,
      check_database: false,
    }
  }

  pub fn components(&self) -> dto::DashboardComponents {
    dto::DashboardComponents {
      items: DASHBOARD_COMPONENTS.to_vec(),
    }
  }

  pub async fn snapshot(&self) -> Result<dto::MonitoringResponse, SumidaError> {
    let system = self.metrics.sample().await?;
    let mut services = vec![dto::ServiceHealth {
      service: "application".to_string(),
      status: dto::HealthStatus::Healthy,
      message: None,
    }];
    if self.check_database {
      services.push(self.database_health().await);
    }

    Ok(dto::MonitoringResponse {
      components: self.components(),
      snapshot: dto::MonitoringSnapshot {
        timestamp: Utc::now(),
        uptime_seconds: system.uptime_seconds,
        resources: dto::ResourceUsage {
          cpu_usage_percent: system.cpu_usage_percent,
          memory_used_bytes: system.memory_used_bytes,
          memory_total_bytes: system.memory_total_bytes,
          load_average: dto::LoadAverage {
            one: system.load_average.one,
            five: system.load_average.five,
            fifteen: system.load_average.fifteen,
          },
        },
        services,
      },
    })
  }

  pub async fn snapshot_only(&self) -> Result<dto::MonitoringSnapshot, SumidaError> {
    Ok(self.snapshot().await?.snapshot)
  }

  async fn database_health(&self) -> dto::ServiceHealth {
    let db = self.state.db();
    let (status, message) =
      match db.execute(Statement::from_string(DatabaseBackend::Postgres, "SELECT 1")).await {
        Ok(_) => (dto::HealthStatus::Healthy, None),
        Err(err) => {
          tracing::error!("database health check failed: {}", err);
          (dto::HealthStatus::Unhealthy, Some(err.to_string()))
        }
      };

    dto::ServiceHealth {
      service: "database".to_string(),
      status,
      message,
    }
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
      load_average: dto::LoadAverage {
        one: 0.1,
        five: 0.2,
        fifteen: 0.3,
      },
    }));

    let service = MonitoringService::with_metrics_provider(DummyState, provider);
    let snapshot = service.snapshot_only().await.unwrap();

    assert_eq!(snapshot.uptime_seconds, 42);
    assert_eq!(snapshot.resources.cpu_usage_percent, 12.5);
    assert_eq!(snapshot.resources.memory_total_bytes, 1024);
    assert_eq!(snapshot.resources.load_average.fifteen, 0.3);
  }
}
