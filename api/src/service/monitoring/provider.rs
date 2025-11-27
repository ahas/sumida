use std::sync::Arc;

use async_trait::async_trait;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::Mutex;

use crate::service::monitoring::dto::LoadAverage;
use crate::service::prelude::*;

pub type DynMetricsProvider = Arc<dyn MetricsProvider>;

#[async_trait]
pub trait MetricsProvider: Send + Sync {
  async fn sample(&self) -> Result<SystemMetrics, SumidaError>;
}

#[derive(Clone, Debug)]
pub struct SystemMetrics {
  pub uptime_seconds:     u64,
  pub cpu_usage_percent:  f32,
  pub memory_used_bytes:  u64,
  pub memory_total_bytes: u64,
  pub load_average:       LoadAverage,
}

#[derive(Clone)]
pub struct SysinfoMetricsProvider {
  system: Arc<Mutex<System>>,
}

impl SysinfoMetricsProvider {
  pub fn new() -> Self {
    let refresh = RefreshKind::new()
      .with_memory(MemoryRefreshKind::new().with_ram())
      .with_cpu(CpuRefreshKind::everything());

    let mut system = System::new_with_specifics(refresh);
    system.refresh_memory();
    system.refresh_cpu_usage();

    Self {
      system: Arc::new(Mutex::new(system)),
    }
  }
}

#[async_trait]
impl MetricsProvider for SysinfoMetricsProvider {
  async fn sample(&self) -> Result<SystemMetrics, SumidaError> {
    let mut system = self.system.lock().await;

    system.refresh_memory();
    system.refresh_cpu_usage();

    let cpu_usage_percent = system.global_cpu_usage();
    let memory_total_bytes = system.total_memory();
    let memory_used_bytes = system.used_memory();
    let load_average = System::load_average();

    Ok(SystemMetrics {
      uptime_seconds: System::uptime(),
      cpu_usage_percent,
      memory_used_bytes,
      memory_total_bytes,
      load_average: LoadAverage {
        one:     load_average.one,
        five:    load_average.five,
        fifteen: load_average.fifteen,
      },
    })
  }
}

#[cfg(test)]
#[derive(Clone)]
pub struct FakeMetricsProvider {
  metrics: SystemMetrics,
}

#[cfg(test)]
impl FakeMetricsProvider {
  pub fn new(metrics: SystemMetrics) -> Self {
    Self { metrics }
  }
}

#[cfg(test)]
#[async_trait]
impl MetricsProvider for FakeMetricsProvider {
  async fn sample(&self) -> Result<SystemMetrics, SumidaError> {
    Ok(self.metrics.clone())
  }
}
