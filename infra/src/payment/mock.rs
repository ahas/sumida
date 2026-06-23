use chrono::Utc;
use serde_json::Value;

use super::{
  PaymentError, PaymentProvider, PaymentStatus, PaymentVerification, RefundResult, WebhookEvent,
  WebhookHeaders,
};

/// Deterministic provider for tests. The merchant order id encodes the outcome:
///
/// - `mock:paid:<amount>[:<currency>]`  → PAID with the given amount
/// - `mock:failed`                      → FAILED
/// - `mock:cancelled`                   → CANCELLED
/// - anything else                      → PAID amount 0 (treated as unverifiable)
///
/// `verify_webhook` skips signature checking and reads `{type, data.paymentId}`
/// straight from the body so webhook handling can be exercised end-to-end.
pub struct MockProvider;

fn parse(payment_id: &str) -> PaymentVerification {
  let parts: Vec<&str> = payment_id.split(':').collect();
  let (status, amount, currency) = if parts.first() == Some(&"mock") {
    let status = match parts.get(1).copied() {
      Some("paid") => PaymentStatus::Paid,
      Some("failed") => PaymentStatus::Failed,
      Some("cancelled") => PaymentStatus::Cancelled,
      _ => PaymentStatus::Unknown,
    };
    let amount = parts.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let currency = parts.get(3).map(|s| s.to_string()).unwrap_or_else(|| "KRW".to_string());
    (status, amount, currency)
  } else {
    (PaymentStatus::Paid, 0, "KRW".to_string())
  };

  PaymentVerification {
    payment_id: payment_id.to_string(),
    transaction_id: Some(format!("mocktxn-{payment_id}")),
    status,
    amount,
    currency,
    paid_at: status.is_paid().then(|| Utc::now().fixed_offset()),
    raw_status: format!("{status:?}"),
  }
}

#[async_trait::async_trait]
impl PaymentProvider for MockProvider {
  fn name(&self) -> &'static str {
    "mock"
  }

  async fn verify(&self, payment_id: &str) -> Result<PaymentVerification, PaymentError> {
    Ok(parse(payment_id))
  }

  async fn cancel(
    &self,
    _payment_id: &str,
    amount: Option<i64>,
    _reason: &str,
  ) -> Result<RefundResult, PaymentError> {
    Ok(RefundResult {
      status: PaymentStatus::Cancelled,
      cancelled_amount: amount.unwrap_or(0),
      raw_status: "CANCELLED".to_string(),
    })
  }

  fn verify_webhook(&self, _headers: &WebhookHeaders, body: &[u8]) -> Result<WebhookEvent, PaymentError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| PaymentError::Other(e.to_string()))?;
    let event_type = v.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let payment_id = v
      .get("data")
      .and_then(|d| d.get("paymentId"))
      .and_then(Value::as_str)
      .ok_or_else(|| PaymentError::Other("webhook missing data.paymentId".into()))?
      .to_string();
    let transaction_id = v
      .get("data")
      .and_then(|d| d.get("transactionId"))
      .and_then(Value::as_str)
      .map(str::to_string);
    Ok(WebhookEvent { event_type, payment_id, transaction_id })
  }
}
