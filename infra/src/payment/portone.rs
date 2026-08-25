//! PortOne (포트원, 구 아임포트) V2 REST API provider.
//!
//! Docs: <https://developers.portone.io/api/rest-v2>.
//! - Auth: `POST {base}/login/api-secret` `{apiSecret}` → `{accessToken}`, then
//!   `Authorization: Bearer {accessToken}`.
//! - Verify: `GET {base}/payments/{paymentId}` → `{status, amount.total, currency, paidAt, ...}`.
//! - Cancel: `POST {base}/payments/{paymentId}/cancel` `{reason, amount?}`.
//! - Webhook: Standard Webhooks (`webhook-id`/`webhook-timestamp`/`webhook-signature`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

use super::{
  CardCredential,
  PaymentError,
  PaymentProvider,
  PaymentStatus,
  PaymentTime,
  PaymentVerification,
  RefundResult,
  WebhookEvent,
  WebhookHeaders,
};
use crate::settings::membership::PaymentSettings;

const DEFAULT_BASE: &str = "https://api.portone.io";

pub struct PortOneProvider {
  client:         reqwest::Client,
  api_base:       String,
  api_secret:     String,
  store_id:       Option<String>,
  channel_key:    Option<String>,
  webhook_secret: Option<String>,
}

impl PortOneProvider {
  pub fn from_settings(cfg: &PaymentSettings) -> Self {
    Self {
      client:         reqwest::Client::new(),
      api_base:       cfg.api_base.clone().unwrap_or_else(|| DEFAULT_BASE.to_string()),
      api_secret:     cfg.api_secret.clone().unwrap_or_default(),
      store_id:       cfg.store_id.clone(),
      channel_key:    cfg.channel_key.clone(),
      webhook_secret: cfg.webhook_secret.clone(),
    }
  }

  /// The moment a payment body says it was paid, if it says so at all.
  ///
  /// The provider writes it in whichever offset it keeps its own books in. What is stored and
  /// answered is the instant, so it is read into UTC here rather than carried on in the offset it
  /// arrived in.
  fn paid_at(value: &Value) -> Option<PaymentTime> {
    value
      .get("paidAt")
      .and_then(Value::as_str)
      .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
      .map(|at| at.with_timezone(&Utc))
  }

  /// Map a PortOne V2 payment JSON object (the body of GET /payments/{id} or the
  /// `payment` field of a billing-key payment response) into a [`PaymentVerification`].
  fn payment_from_json(value: &Value, fallback_id: &str, fallback_currency: &str) -> PaymentVerification {
    let raw_status = value.get("status").and_then(Value::as_str).unwrap_or("").to_string();
    let amount = value.get("amount").and_then(|a| a.get("total")).and_then(Value::as_i64).unwrap_or(0);
    let currency = value.get("currency").and_then(Value::as_str).unwrap_or(fallback_currency).to_string();
    let paid_at = Self::paid_at(value);
    PaymentVerification {
      payment_id: value.get("id").and_then(Value::as_str).unwrap_or(fallback_id).to_string(),
      transaction_id: value.get("transactionId").and_then(Value::as_str).map(str::to_string),
      status: PaymentStatus::from_portone(&raw_status),
      amount,
      currency,
      paid_at,
      raw_status,
    }
  }

  /// Exchange the API secret for a short-lived Bearer access token.
  async fn token(&self) -> Result<String, PaymentError> {
    let resp = self
      .client
      .post(format!("{}/login/api-secret", self.api_base))
      .json(&json!({ "apiSecret": self.api_secret }))
      .send()
      .await
      .map_err(|e| PaymentError::Request(e.to_string()))?;

    if !resp.status().is_success() {
      return Err(PaymentError::Request(format!("token request failed: {}", resp.status())));
    }
    let body: Value = resp.json().await.map_err(|e| PaymentError::Request(e.to_string()))?;
    body
      .get("accessToken")
      .and_then(Value::as_str)
      .map(str::to_string)
      .ok_or_else(|| PaymentError::Other("login response missing accessToken".into()))
  }
}

#[async_trait::async_trait]
impl PaymentProvider for PortOneProvider {
  fn name(&self) -> &'static str {
    "portone"
  }

  async fn verify(&self, payment_id: &str) -> Result<PaymentVerification, PaymentError> {
    let token = self.token().await?;
    let url = format!("{}/payments/{}", self.api_base, urlencoding(payment_id));
    let resp = self
      .client
      .get(url)
      .bearer_auth(token)
      .send()
      .await
      .map_err(|e| PaymentError::Request(e.to_string()))?;

    if resp.status().as_u16() == 404 {
      return Err(PaymentError::NotFound(payment_id.to_string()));
    }
    if !resp.status().is_success() {
      return Err(PaymentError::Request(format!("verify failed: {}", resp.status())));
    }

    let v: Value = resp.json().await.map_err(|e| PaymentError::Request(e.to_string()))?;
    Ok(Self::payment_from_json(&v, payment_id, "KRW"))
  }

  async fn cancel(
    &self,
    payment_id: &str,
    amount: Option<i64>,
    reason: &str,
  ) -> Result<RefundResult, PaymentError> {
    let token = self.token().await?;
    let url = format!("{}/payments/{}/cancel", self.api_base, urlencoding(payment_id));

    let mut body = json!({ "reason": reason });
    if let Some(a) = amount {
      body["amount"] = json!(a);
    }
    if let Some(store) = &self.store_id {
      body["storeId"] = json!(store);
    }

    let resp = self
      .client
      .post(url)
      .bearer_auth(token)
      .json(&body)
      .send()
      .await
      .map_err(|e| PaymentError::Request(e.to_string()))?;

    if !resp.status().is_success() {
      return Err(PaymentError::Request(format!("cancel failed: {}", resp.status())));
    }

    let v: Value = resp.json().await.map_err(|e| PaymentError::Request(e.to_string()))?;
    // Response shape: { cancellation: { totalAmount, status, ... } }. The cancellation `status` is the
    // *operation* result (always "SUCCEEDED" on a 2xx) — NOT the payment's post-cancel state. Whether
    // the payment is now PARTIAL_CANCELLED vs CANCELLED is only visible via verify(); here we just
    // report that the cancellation succeeded and how much it cancelled.
    let cancellation = v.get("cancellation");
    let cancelled_amount = cancellation
      .and_then(|c| c.get("totalAmount"))
      .and_then(Value::as_i64)
      .or(amount)
      .unwrap_or(0);
    let raw_status = cancellation
      .and_then(|c| c.get("status"))
      .and_then(Value::as_str)
      .unwrap_or("SUCCEEDED")
      .to_string();

    Ok(RefundResult {
      status: PaymentStatus::Cancelled,
      cancelled_amount,
      raw_status,
    })
  }

  async fn charge_recurring(
    &self,
    billing_key: &str,
    amount: i64,
    currency: &str,
    order_code: &str,
  ) -> Result<PaymentVerification, PaymentError> {
    // PortOne V2 billing-key (정기결제) payment:
    //   POST {base}/payments/{paymentId}/billing-key
    //   body: { storeId?, channelKey?, billingKey, orderName, amount: { total }, currency }
    //   → PayWithBillingKeyResponse { payment: {…same shape as GET /payments/{id}…} }
    // Issuing a billing key (POST /billing-keys) is a separate client/SDK step.
    let token = self.token().await?;
    let url = format!("{}/payments/{}/billing-key", self.api_base, urlencoding(order_code));

    let mut body = json!({
      "billingKey": billing_key,
      "orderName": order_code,
      "amount": { "total": amount },
      "currency": currency,
    });
    if let Some(store) = &self.store_id {
      body["storeId"] = json!(store);
    }
    if let Some(channel) = &self.channel_key {
      body["channelKey"] = json!(channel);
    }

    let resp = self
      .client
      .post(url)
      .bearer_auth(token)
      .json(&body)
      .send()
      .await
      .map_err(|e| PaymentError::Request(e.to_string()))?;

    // A declined or rejected charge returns a non-2xx error body; surface it as a
    // request error so the auto-renew/dunning caller treats it as a failed charge.
    if !resp.status().is_success() {
      let status = resp.status();
      let detail = resp.text().await.unwrap_or_default();
      return Err(PaymentError::Request(format!("billing-key payment failed ({status}): {detail}")));
    }

    let v: Value = resp.json().await.map_err(|e| PaymentError::Request(e.to_string()))?;
    // PortOne V2's billing-key payment success returns a minimal `{ payment: { paidAt, pgTxId } }`
    // (no status/amount). A present `paidAt` means PAID; fall back to the full shape otherwise.
    let payment = v.get("payment").unwrap_or(&v);
    let paid_at = Self::paid_at(payment);
    if paid_at.is_some() {
      return Ok(PaymentVerification {
        payment_id: order_code.to_string(),
        transaction_id: payment.get("pgTxId").and_then(Value::as_str).map(str::to_string),
        status: PaymentStatus::Paid,
        amount: payment
          .get("amount")
          .and_then(|a| a.get("total"))
          .and_then(Value::as_i64)
          .unwrap_or(amount),
        currency: currency.to_string(),
        paid_at,
        raw_status: "PAID".to_string(),
      });
    }
    Ok(Self::payment_from_json(payment, order_code, currency))
  }

  async fn verify_billing_key(&self, billing_key: &str) -> Result<(), PaymentError> {
    // PortOne V2: GET {base}/billing-keys/{billingKey} → { status: ISSUED|DELETED, … }.
    let token = self.token().await?;
    let url = format!("{}/billing-keys/{}", self.api_base, urlencoding(billing_key));
    let resp = self
      .client
      .get(url)
      .bearer_auth(token)
      .send()
      .await
      .map_err(|e| PaymentError::Request(e.to_string()))?;

    if resp.status().as_u16() == 404 {
      return Err(PaymentError::NotFound(billing_key.to_string()));
    }
    if !resp.status().is_success() {
      return Err(PaymentError::Request(format!("billing-key lookup failed: {}", resp.status())));
    }
    let v: Value = resp.json().await.map_err(|e| PaymentError::Request(e.to_string()))?;
    let status = v.get("status").and_then(Value::as_str).unwrap_or("");
    if status.eq_ignore_ascii_case("DELETED") {
      return Err(PaymentError::NotFound(billing_key.to_string()));
    }
    Ok(())
  }

  async fn issue_billing_key(
    &self,
    card: &CardCredential,
    customer_id: &str,
  ) -> Result<String, PaymentError> {
    // PortOne V2: POST {base}/billing-keys with keyin card credential. The PG must allow keyin.
    let channel_key = self.channel_key.as_ref().ok_or(PaymentError::NotConfigured)?;
    let token = self.token().await?;

    let mut body = json!({
      "channelKey": channel_key,
      "method": { "card": { "credential": {
        "number": card.number,
        "expiryYear": card.expiry_year,
        "expiryMonth": card.expiry_month,
        "birthOrBusinessRegistrationNumber": card.birth_or_business_registration_number,
        "passwordTwoDigits": card.password_two_digits,
      } } },
      "customer": { "id": customer_id },
    });
    if let Some(store) = &self.store_id {
      body["storeId"] = json!(store);
    }

    let resp = self
      .client
      .post(format!("{}/billing-keys", self.api_base))
      .bearer_auth(token)
      .json(&body)
      .send()
      .await
      .map_err(|e| PaymentError::Request(e.to_string()))?;

    let status = resp.status();
    let v: Value = resp.json().await.map_err(|e| PaymentError::Request(e.to_string()))?;
    if !status.is_success() {
      return Err(PaymentError::Request(format!("billing-key issue failed: {status} {v}")));
    }

    // V2 response: { billingKeyInfo: { billingKey, ... } } (fall back to a top-level billingKey).
    v.get("billingKeyInfo")
      .and_then(|b| b.get("billingKey"))
      .and_then(Value::as_str)
      .or_else(|| v.get("billingKey").and_then(Value::as_str))
      .map(str::to_string)
      .ok_or_else(|| PaymentError::Other(format!("issue response missing billingKey: {v}")))
  }

  fn verify_webhook(&self, headers: &WebhookHeaders, body: &[u8]) -> Result<WebhookEvent, PaymentError> {
    let secret = self.webhook_secret.as_ref().ok_or(PaymentError::NotConfigured)?;
    verify_standard_webhook(secret, headers, body)?;

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

/// Minimal percent-encoding for a path segment (payment ids are usually safe,
/// but encode the reserved characters that can appear).
fn urlencoding(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for b in s.bytes() {
    match b {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
      _ => out.push_str(&format!("%{b:02X}")),
    }
  }
  out
}

/// Verify a Standard Webhooks signature (HMAC-SHA256 over `id.timestamp.body`).
fn verify_standard_webhook(
  secret: &str,
  headers: &WebhookHeaders,
  body: &[u8],
) -> Result<(), PaymentError> {
  let key = secret.strip_prefix("whsec_").unwrap_or(secret);
  let key_bytes = BASE64.decode(key).map_err(|_| PaymentError::InvalidSignature)?;
  let body_str = std::str::from_utf8(body).map_err(|_| PaymentError::InvalidSignature)?;
  let signed = format!("{}.{}.{}", headers.id, headers.timestamp, body_str);

  let mut mac = Hmac::<Sha256>::new_from_slice(&key_bytes).map_err(|_| PaymentError::InvalidSignature)?;
  mac.update(signed.as_bytes());
  let expected = BASE64.encode(mac.finalize().into_bytes());

  // `webhook-signature` is a space-separated list of `v1,<base64sig>` entries.
  let matched = headers.signature.split(' ').any(|part| {
    let sig = part.split_once(',').map(|(_, s)| s).unwrap_or(part);
    constant_time_eq(sig.as_bytes(), expected.as_bytes())
  });

  if matched { Ok(()) } else { Err(PaymentError::InvalidSignature) }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
  if a.len() != b.len() {
    return false;
  }
  let mut diff = 0u8;
  for (x, y) in a.iter().zip(b.iter()) {
    diff |= x ^ y;
  }
  diff == 0
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn maps_paid_payment_json() {
    let v = serde_json::json!({
      "id": "pay_123",
      "status": "PAID",
      "amount": { "total": 5000, "taxFree": 0, "vat": 0 },
      "currency": "KRW",
      "paidAt": "2026-06-27T00:00:00Z",
      "transactionId": "txn_abc",
    });
    let pv = PortOneProvider::payment_from_json(&v, "order_1", "KRW");
    assert!(pv.status.is_paid());
    assert_eq!(pv.amount, 5000);
    assert_eq!(pv.currency, "KRW");
    assert_eq!(pv.transaction_id.as_deref(), Some("txn_abc"));
    assert_eq!(pv.payment_id, "pay_123");
    assert!(pv.paid_at.is_some());
  }

  #[test]
  fn falls_back_when_fields_missing() {
    // A non-paid response with sparse fields uses the supplied fallbacks.
    let v = serde_json::json!({ "status": "FAILED" });
    let pv = PortOneProvider::payment_from_json(&v, "order_2", "USD");
    assert!(!pv.status.is_paid());
    assert_eq!(pv.amount, 0);
    assert_eq!(pv.currency, "USD");
    assert_eq!(pv.payment_id, "order_2");
    assert!(pv.transaction_id.is_none());
  }
}

// Live smoke test: confirm the V2 API Secret authenticates (the /login/api-secret token call). Needs
// no real payment — verify_billing_key on a nonexistent key proves token auth works (-> NotFound),
// vs an auth failure (-> Request). Reads SUMIDA_TEST_PORTONE_SECRET (+ optional
// SUMIDA_TEST_PORTONE_STORE); skipped unless the secret is set, so it never runs in normal CI.
#[cfg(test)]
mod live_tests {
  use super::PortOneProvider;
  use crate::payment::{PaymentError, PaymentProvider, PaymentStatus};
  use crate::settings::membership::PaymentSettings;

  #[tokio::test]
  async fn portone_api_secret_authenticates() {
    let Ok(api_secret) = std::env::var("SUMIDA_MEMBERSHIP__PAYMENT__API_SECRET") else {
      eprintln!("skipping: set SUMIDA_MEMBERSHIP__PAYMENT__API_SECRET to run the PortOne auth smoke test");
      return;
    };
    let cfg = PaymentSettings {
      provider:       "portone".to_string(),
      api_base:       None,
      api_secret:     Some(api_secret),
      store_id:       std::env::var("SUMIDA_MEMBERSHIP__PAYMENT__STORE_ID").ok(),
      channel_key:    None,
      webhook_secret: None,
    };
    let provider = PortOneProvider::from_settings(&cfg);

    match provider.verify_billing_key("does-not-exist-smoke-test").await {
      Err(PaymentError::NotFound(_)) => {} // token auth succeeded; the key simply doesn't exist
      Ok(()) => panic!("unexpected: a nonexistent billing key reported valid"),
      Err(e) => panic!("PortOne auth/request failed — check the API secret: {e}"),
    }
  }

  // Full recurring flow against real PortOne+PG: keyin billing-key issuance -> verify -> charge.
  // Uses the configured PortOne creds (SUMIDA_MEMBERSHIP__PAYMENT__API_SECRET / _STORE_ID /
  // _CHANNEL_KEY — must be a recurring channel) plus a test card via SUMIDA_TEST_CARD_* (Toss test
  // mode only validates the BIN/first 6 digits). Skipped unless SUMIDA_TEST_CARD_NUMBER is set.
  #[tokio::test]
  async fn portone_keyin_billing_and_recurring() {
    use super::super::CardCredential;

    let Ok(number) = std::env::var("SUMIDA_TEST_CARD_NUMBER") else {
      eprintln!("skipping: set SUMIDA_TEST_CARD_NUMBER (+ SUMIDA_MEMBERSHIP__PAYMENT__* creds) to run");
      return;
    };
    let env = |k: &str| std::env::var(k).unwrap_or_default();

    let cfg = PaymentSettings {
      provider:       "portone".to_string(),
      api_base:       None,
      api_secret:     std::env::var("SUMIDA_MEMBERSHIP__PAYMENT__API_SECRET").ok(),
      store_id:       std::env::var("SUMIDA_MEMBERSHIP__PAYMENT__STORE_ID").ok(),
      channel_key:    std::env::var("SUMIDA_MEMBERSHIP__PAYMENT__CHANNEL_KEY").ok(),
      webhook_secret: None,
    };
    let provider = PortOneProvider::from_settings(&cfg);

    let card = CardCredential {
      number,
      expiry_year: env("SUMIDA_TEST_CARD_EXP_YEAR"),
      expiry_month: env("SUMIDA_TEST_CARD_EXP_MONTH"),
      birth_or_business_registration_number: env("SUMIDA_TEST_CARD_BIRTH"),
      password_two_digits: env("SUMIDA_TEST_CARD_PW2"),
    };

    // 1) issue a billing key (server-side keyin)
    let billing_key = provider
      .issue_billing_key(&card, "test-customer-1")
      .await
      .expect("billing-key issuance should succeed");

    // 2) verify it's active
    provider
      .verify_billing_key(&billing_key)
      .await
      .expect("issued key should verify as active");

    // 3) charge it (1000 KRW test charge)
    let order = format!("test-recur-{}", std::time::SystemTime::UNIX_EPOCH.elapsed().unwrap().as_millis());
    let result = provider
      .charge_recurring(&billing_key, 1000, "KRW", &order)
      .await
      .expect("recurring charge request should succeed");
    assert!(result.status.is_paid(), "recurring charge should be paid, got {:?}", result.status);

    // 4) the charge is retrievable (exercises the real verify / payment_from_json parser)
    let fetched = provider.verify(&order).await.expect("charge should be verifiable");
    assert!(fetched.status.is_paid(), "verified charge should be paid, got {:?}", fetched.status);
    assert_eq!(fetched.amount, 1000, "verified amount should match the charge");

    // 5) partial refund (400 of 1000): the cancel succeeds for 400, and the payment itself flips to
    //    PARTIAL_CANCELLED (visible via verify, not via the cancel response's operation status).
    let partial = provider.cancel(&order, Some(400), "partial test refund").await.expect("partial refund");
    assert_eq!(partial.cancelled_amount, 400, "partial cancel amount");
    let after_partial = provider.verify(&order).await.expect("verify after partial");
    assert_eq!(
      after_partial.status,
      PaymentStatus::PartialCancelled,
      "payment should be partially cancelled, got {:?}",
      after_partial.status
    );

    // 6) refund the remaining 600 -> the payment is now fully CANCELLED.
    let rest = provider
      .cancel(&order, Some(600), "remaining test refund")
      .await
      .expect("remaining refund");
    assert_eq!(rest.cancelled_amount, 600, "remaining cancel amount");
    let after_full = provider.verify(&order).await.expect("verify after full");
    assert_eq!(
      after_full.status,
      PaymentStatus::Cancelled,
      "payment should be fully cancelled, got {:?}",
      after_full.status
    );
  }
}
