use axum::Json;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use sea_orm::ConnectionTrait;
use serde::Serialize;
use sysinfo::System;

use crate::service::monitoring::provider::SystemMetrics;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardComponent {
  pub key: &'static str,
  pub title: &'static str,
  pub description: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardComponents {
  pub items: Vec<DashboardComponent>,
}

impl IntoResponse for DashboardComponents {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
  pub swap_total: u64,
  pub swap_used: u64,
  pub num_cpus: usize,
  pub name: Option<String>,
  pub kernel_version: Option<String>,
  pub os_version: Option<String>,
  pub host_name: Option<String>,
}

impl SystemInfo {
  pub fn new(system: System) -> Self {
    Self {
      swap_total: system.total_swap(),
      swap_used: system.used_swap(),
      num_cpus: system.cpus().len(),
      name: System::name(),
      kernel_version: System::kernel_version(),
      os_version: System::long_os_version(),
      host_name: System::host_name(),
    }
  }
}

impl IntoResponse for SystemInfo {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadAverage {
  pub one: f64,
  pub five: f64,
  pub fifteen: f64,
}

impl LoadAverage {
  pub fn new(system: SystemMetrics) -> Self {
    Self {
      one: system.load_average.one,
      five: system.load_average.five,
      fifteen: system.load_average.fifteen,
    }
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
  pub cpu_usage_percent: f32,
  pub memory_used_bytes: u64,
  pub memory_total_bytes: u64,
  pub load_average: LoadAverage,
}

impl ResourceUsage {
  pub fn new(system: SystemMetrics) -> Self {
    Self {
      cpu_usage_percent: system.cpu_usage_percent,
      memory_used_bytes: system.memory_used_bytes,
      memory_total_bytes: system.memory_total_bytes,
      load_average: LoadAverage::new(system),
    }
  }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
  Healthy,
  Degraded,
  Unhealthy,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
  pub service: String,
  pub status: HealthStatus,
  pub message: Option<String>,
}

impl ServiceHealth {
  pub fn new_for_app() -> Self {
    Self {
      service: "application".to_string(),
      status: HealthStatus::Healthy,
      message: None,
    }
  }

  pub async fn new_for_db<'db, C: ConnectionTrait>(db: &'db C) -> Self {
    use sea_orm::Statement;

    let (status, message) =
      match db.execute(Statement::from_string(db.get_database_backend(), "SELECT 1")).await {
        Ok(_) => (HealthStatus::Healthy, None),
        Err(err) => {
          tracing::error!("database health check failed: {}", err);
          (HealthStatus::Unhealthy, Some(err.to_string()))
        }
      };

    Self {
      service: "database".to_string(),
      status,
      message,
    }
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSnapshot {
  pub timestamp: DateTime<Utc>,
  pub uptime_seconds: u64,
  pub resources: ResourceUsage,
  pub services: Vec<ServiceHealth>,
}

impl MonitoringSnapshot {
  pub fn new(system: SystemMetrics, services: Vec<ServiceHealth>) -> Self {
    Self {
      timestamp: Utc::now(),
      uptime_seconds: system.uptime_seconds,
      resources: ResourceUsage::new(system),
      services,
    }
  }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringResponse {
  pub components: DashboardComponents,
  pub system_info: SystemInfo,
  pub snapshot: MonitoringSnapshot,
}

impl IntoResponse for MonitoringResponse {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}
