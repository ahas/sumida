use chrono::{DateTime, Utc};
use serde::Serialize;

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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadAverage {
  pub one: f64,
  pub five: f64,
  pub fifteen: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
  pub cpu_usage_percent: f32,
  pub memory_used_bytes: u64,
  pub memory_total_bytes: u64,
  pub load_average: LoadAverage,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSnapshot {
  pub timestamp: DateTime<Utc>,
  pub uptime_seconds: u64,
  pub resources: ResourceUsage,
  pub services: Vec<ServiceHealth>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringResponse {
  pub components: DashboardComponents,
  pub snapshot: MonitoringSnapshot,
}
