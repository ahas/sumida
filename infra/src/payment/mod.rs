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
  pub payment_id:     String,
  /// PG-side transaction id, when available.
  pub transaction_id: Option<String>,
  pub status:         PaymentStatus,
  /// Total paid amount in minor currency units.
  pub amount:         i64,
  pub currency:       String,
  pub paid_at:        Option<PaymentTime>,
  pub raw_status:     String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefundResult {
  pub status:           PaymentStatus,
  pub cancelled_amount: i64,
  pub raw_status:       String,
}

/// Result of a two-phase `authorize` (hold without capturing). `authorization_id` is the PG-side
/// handle (when distinct from `payment_id`) used by a later `capture`/`void`.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorizeResult {
  pub payment_id:       String,
  pub authorization_id: String,
  pub status:           PaymentStatus,
  pub amount:           i64,
  pub currency:         String,
  pub raw_status:       String,
}

/// Result of capturing a prior authorization.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureResult {
  pub payment_id:      String,
  pub status:          PaymentStatus,
  pub captured_amount: i64,
  pub raw_status:      String,
}

/// Result of voiding a prior authorization without capturing.
#[derive(Debug, Clone, Serialize)]
pub struct VoidResult {
  pub payment_id: String,
  pub status:     PaymentStatus,
  pub raw_status: String,
}

/// Raw card credential for **server-side (keyin) billing-key issuance**. The PG (not our server)
/// retains the card; this only transits to the PG via the provider. Production keyin needs PCI scope
/// + PG enablement — primarily used to automate recurring tests with a test card.
#[derive(Debug, Clone)]
pub struct CardCredential {
  pub number: String,
  /// Two-digit year, e.g. "29".
  pub expiry_year: String,
  /// Two-digit month, e.g. "12".
  pub expiry_month: String,
  /// Birthdate (YYMMDD) for personal cards, or the business registration number.
  pub birth_or_business_registration_number: String,
  /// First two digits of the card password.
  pub password_two_digits: String,
}

/// Standard-Webhooks signature headers.
#[derive(Debug, Clone, Default)]
pub struct WebhookHeaders {
  pub id:        String,
  pub timestamp: String,
  pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookEvent {
  /// e.g. `Transaction.Paid`, `Transaction.Cancelled`.
  pub event_type:     String,
  /// Merchant order id (`data.paymentId`).
  pub payment_id:     String,
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

  /// Charge a previously issued billing key for a recurring payment (정기결제).
  /// `order_code` becomes the merchant order id. Returns the normalized result;
  /// callers must check [`PaymentVerification::status`] (a non-paid status is a
  /// decline, not an error). Providers without billing-key support return
  /// [`PaymentError::Unsupported`].
  async fn charge_recurring(
    &self,
    billing_key: &str,
    amount: i64,
    currency: &str,
    order_code: &str,
  ) -> Result<PaymentVerification, PaymentError>;

  /// Verify a billing key exists and is active at the PG before storing it for
  /// recurring charges (issued client-side via the SDK, then injected here).
  /// `Ok(())` = usable; `Err(NotFound)` = invalid/inactive; `Err(NotConfigured)`
  /// when no PG is configured.
  async fn verify_billing_key(&self, billing_key: &str) -> Result<(), PaymentError>;

  /// Verify a webhook signature and extract the normalized event.
  fn verify_webhook(&self, headers: &WebhookHeaders, body: &[u8]) -> Result<WebhookEvent, PaymentError>;

  // ---- Two-phase (authorize → capture/void): the PG-side analog of credit reserve/capture/release.
  // Default to `Unsupported`; only PGs with manual-capture override these.

  /// Authorize (hold) `amount` without capturing — like a credit `reserve`. The returned
  /// `authorization_id` is captured/voided later.
  async fn authorize(
    &self,
    payment_id: &str,
    amount: i64,
    currency: &str,
  ) -> Result<AuthorizeResult, PaymentError> {
    let _ = (payment_id, amount, currency);
    Err(PaymentError::Unsupported)
  }

  /// Capture a prior [`Self::authorize`] (full when `amount = None`) — like a credit `capture`.
  async fn capture(&self, payment_id: &str, amount: Option<i64>) -> Result<CaptureResult, PaymentError> {
    let _ = (payment_id, amount);
    Err(PaymentError::Unsupported)
  }

  /// Void a prior [`Self::authorize`] without capturing — like a credit `release`.
  async fn void(&self, payment_id: &str) -> Result<VoidResult, PaymentError> {
    let _ = payment_id;
    Err(PaymentError::Unsupported)
  }

  /// Issue a billing key server-side (keyin) from raw card credentials. Normally billing keys come
  /// from the browser SDK; this is the API path (PG must allow keyin). Returns the billing key.
  async fn issue_billing_key(
    &self,
    card: &CardCredential,
    customer_id: &str,
  ) -> Result<String, PaymentError> {
    let _ = (card, customer_id);
    Err(PaymentError::Unsupported)
  }
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
