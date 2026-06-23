//! Payment-gateway (PG) abstraction.
//!
//! The membership domain never talks to a PG directly — it goes through the
//! [`PaymentProvider`] trait, which returns normalized, PG-agnostic results.
//! PortOne (포트원, 구 아임포트) is the first-class implementation; additional
//! PGs (Toss, Kakao, Stripe, …) only need to implement the same trait.

pub mod mock;
pub mod none;
pub mod portone;

use std::sync::{Arc, OnceLock};

use chrono::{DateTime, FixedOffset};
use serde::Serialize;

use crate::settings;

pub type PaymentTime = DateTime<FixedOffset>;

/// Normalized payment status across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
  Ready,
  Pending,
  Paid,
  Failed,
  Cancelled,
  PartialCancelled,
  VirtualAccountIssued,
  Unknown,
}

impl PaymentStatus {
  pub fn is_paid(self) -> bool {
    matches!(self, Self::Paid)
  }

  /// Map PortOne V2 `PaymentStatus` strings.
  pub fn from_portone(s: &str) -> Self {
    match s {
      "READY" => Self::Ready,
      "PENDING" => Self::Pending,
      "PAID" => Self::Paid,
      "FAILED" => Self::Failed,
      "CANCELLED" => Self::Cancelled,
      "PARTIAL_CANCELLED" => Self::PartialCancelled,
      "VIRTUAL_ACCOUNT_ISSUED" => Self::VirtualAccountIssued,
      _ => Self::Unknown,
    }
  }
}

/// Result of verifying a payment server-side (the authoritative amount/status).
#[derive(Debug, Clone, Serialize)]
pub struct PaymentVerification {
  /// Merchant order id (PortOne V2 `paymentId`) = our `membership_purchases.code`.
  pub payment_id: String,
  /// PG-side transaction id, when available.
  pub transaction_id: Option<String>,
  pub status: PaymentStatus,
  /// Total paid amount in minor currency units.
  pub amount: i64,
  pub currency: String,
  pub paid_at: Option<PaymentTime>,
  pub raw_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefundResult {
  pub status: PaymentStatus,
  pub cancelled_amount: i64,
  pub raw_status: String,
}

/// Standard-Webhooks signature headers.
#[derive(Debug, Clone, Default)]
pub struct WebhookHeaders {
  pub id: String,
  pub timestamp: String,
  pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookEvent {
  /// e.g. `Transaction.Paid`, `Transaction.Cancelled`.
  pub event_type: String,
  /// Merchant order id (`data.paymentId`).
  pub payment_id: String,
  pub transaction_id: Option<String>,
}

impl WebhookEvent {
  pub fn is_paid(&self) -> bool {
    self.event_type.eq_ignore_ascii_case("Transaction.Paid")
  }
  pub fn is_cancelled(&self) -> bool {
    self.event_type.eq_ignore_ascii_case("Transaction.Cancelled")
  }
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
  #[error("payment provider request failed: {0}")]
  Request(String),
  #[error("payment `{0}` not found at provider")]
  NotFound(String),
  #[error("webhook signature verification failed")]
  InvalidSignature,
  #[error("payment provider is not configured")]
  NotConfigured,
  #[error("operation not supported by this provider")]
  Unsupported,
  #[error("payment provider error: {0}")]
  Other(String),
}

#[async_trait::async_trait]
pub trait PaymentProvider: Send + Sync {
  fn name(&self) -> &'static str;

  /// Server-side verification of a payment by its merchant order id.
  async fn verify(&self, payment_id: &str) -> Result<PaymentVerification, PaymentError>;

  /// Cancel (full or partial) a payment. `amount = None` cancels in full.
  async fn cancel(
    &self,
    payment_id: &str,
    amount: Option<i64>,
    reason: &str,
  ) -> Result<RefundResult, PaymentError>;

  /// Verify a webhook signature and extract the normalized event.
  fn verify_webhook(&self, headers: &WebhookHeaders, body: &[u8]) -> Result<WebhookEvent, PaymentError>;
}

static PROVIDER: OnceLock<Arc<dyn PaymentProvider>> = OnceLock::new();

/// The process-wide configured payment provider (built once from settings).
pub fn provider() -> Arc<dyn PaymentProvider> {
  PROVIDER.get_or_init(build_from_settings).clone()
}

fn build_from_settings() -> Arc<dyn PaymentProvider> {
  let cfg = &settings::get().membership.payment;
  match cfg.provider.as_str() {
    "portone" => Arc::new(portone::PortOneProvider::from_settings(cfg)),
    "mock" => Arc::new(mock::MockProvider),
    _ => Arc::new(none::NoneProvider),
  }
}
