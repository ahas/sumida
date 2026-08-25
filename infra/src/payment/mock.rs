use chrono::Utc;
use serde_json::Value;

use super::{
  AuthorizeResult,
  CaptureResult,
  CardCredential,
  PaymentError,
  PaymentProvider,
  PaymentStatus,
  PaymentVerification,
  RefundResult,
  VoidResult,
  WebhookEvent,
  WebhookHeaders,
};

/// Deterministic provider for tests. The merchant order id encodes the outcome:
///
/// - `mock:paid:<amount>[:<currency>]`  → PAID with the given amount
/// - `mock:failed`                      → FAILED
/// - `mock:cancelled`                   → CANCELLED
/// - anything else                      → PAID amount 0 (treated as unverifiable)
///
/// For recurring charges the billing key encodes the outcome:
/// - `mock:bill:ok`   → PAID with the requested amount
/// - anything else    → FAILED (a declined recurring charge)
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
    paid_at: status.is_paid().then(Utc::now),
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
      status:           PaymentStatus::Cancelled,
      cancelled_amount: amount.unwrap_or(0),
      raw_status:       "CANCELLED".to_string(),
    })
  }

  async fn charge_recurring(
    &self,
    billing_key: &str,
    amount: i64,
    currency: &str,
    order_code: &str,
  ) -> Result<PaymentVerification, PaymentError> {
    let status = if billing_key == "mock:bill:ok" { PaymentStatus::Paid } else { PaymentStatus::Failed };
    Ok(PaymentVerification {
      payment_id: order_code.to_string(),
      transaction_id: Some(format!("mockbill-{order_code}")),
      status,
      amount,
      currency: currency.to_string(),
      paid_at: status.is_paid().then(Utc::now),
      raw_status: format!("{status:?}"),
    })
  }

  async fn verify_billing_key(&self, billing_key: &str) -> Result<(), PaymentError> {
    // Any `mock:bill:*` is a "real" key (issuance succeeded); the charge outcome
    // — paid vs declined — is decided separately in `charge_recurring`.
    if billing_key.starts_with("mock:bill:") {
      Ok(())
    } else {
      Err(PaymentError::NotFound(billing_key.to_string()))
    }
  }

  async fn authorize(
    &self,
    payment_id: &str,
    amount: i64,
    currency: &str,
  ) -> Result<AuthorizeResult, PaymentError> {
    Ok(AuthorizeResult {
      payment_id: payment_id.to_string(),
      authorization_id: format!("mockauth-{payment_id}"),
      status: PaymentStatus::Ready, // held, awaiting capture
      amount,
      currency: currency.to_string(),
      raw_status: "AUTHORIZED".to_string(),
    })
  }

  async fn capture(&self, payment_id: &str, amount: Option<i64>) -> Result<CaptureResult, PaymentError> {
    Ok(CaptureResult {
      payment_id:      payment_id.to_string(),
      status:          PaymentStatus::Paid,
      captured_amount: amount.unwrap_or(0),
      raw_status:      "PAID".to_string(),
    })
  }

  async fn void(&self, payment_id: &str) -> Result<VoidResult, PaymentError> {
    Ok(VoidResult {
      payment_id: payment_id.to_string(),
      status:     PaymentStatus::Cancelled,
      raw_status: "CANCELLED".to_string(),
    })
  }

  async fn issue_billing_key(
    &self,
    _card: &CardCredential,
    _customer_id: &str,
  ) -> Result<String, PaymentError> {
    // A "real" issued key in the mock world (charge outcome is decided by `charge_recurring`).
    Ok("mock:bill:ok".to_string())
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
    Ok(WebhookEvent {
      event_type,
      payment_id,
      transaction_id,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::super::none::NoneProvider;
  use super::*;

  #[tokio::test]
  async fn mock_two_phase_authorize_capture_void() {
    let p = MockProvider;

    let auth = p.authorize("ord-1", 1000, "KRW").await.unwrap();
    assert_eq!(auth.amount, 1000);
    assert_eq!(auth.status, PaymentStatus::Ready);
    assert!(auth.authorization_id.contains("ord-1"));

    let cap = p.capture("ord-1", Some(1000)).await.unwrap();
    assert_eq!(cap.status, PaymentStatus::Paid);
    assert_eq!(cap.captured_amount, 1000);

    let voided = p.void("ord-2").await.unwrap();
    assert_eq!(voided.status, PaymentStatus::Cancelled);
  }

  #[tokio::test]
  async fn providers_without_manual_capture_are_unsupported() {
    let n = NoneProvider;
    assert!(matches!(n.authorize("x", 1, "KRW").await, Err(PaymentError::Unsupported)));
    assert!(matches!(n.capture("x", None).await, Err(PaymentError::Unsupported)));
    assert!(matches!(n.void("x").await, Err(PaymentError::Unsupported)));
  }
}
