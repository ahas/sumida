use super::{
  PaymentError, PaymentProvider, PaymentVerification, RefundResult, WebhookEvent, WebhookHeaders,
};

/// Default provider when payments are disabled. Every operation reports
/// `NotConfigured`, so the PG endpoints fail safely until a provider is set.
pub struct NoneProvider;

#[async_trait::async_trait]
impl PaymentProvider for NoneProvider {
  fn name(&self) -> &'static str {
    "none"
  }

  async fn verify(&self, _payment_id: &str) -> Result<PaymentVerification, PaymentError> {
    Err(PaymentError::NotConfigured)
  }

  async fn cancel(
    &self,
    _payment_id: &str,
    _amount: Option<i64>,
    _reason: &str,
  ) -> Result<RefundResult, PaymentError> {
    Err(PaymentError::NotConfigured)
  }

  async fn charge_recurring(
    &self,
    _billing_key: &str,
    _amount: i64,
    _currency: &str,
    _order_code: &str,
  ) -> Result<PaymentVerification, PaymentError> {
    Err(PaymentError::NotConfigured)
  }

  fn verify_webhook(&self, _headers: &WebhookHeaders, _body: &[u8]) -> Result<WebhookEvent, PaymentError> {
    Err(PaymentError::NotConfigured)
  }
}
