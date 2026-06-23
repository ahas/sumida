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
use chrono::DateTime;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

use super::{
  PaymentError, PaymentProvider, PaymentStatus, PaymentVerification, RefundResult, WebhookEvent,
  WebhookHeaders,
};
use crate::settings::membership::PaymentSettings;

const DEFAULT_BASE: &str = "https://api.portone.io";

pub struct PortOneProvider {
  client: reqwest::Client,
  api_base: String,
  api_secret: String,
  store_id: Option<String>,
  webhook_secret: Option<String>,
}

impl PortOneProvider {
  pub fn from_settings(cfg: &PaymentSettings) -> Self {
    Self {
      client: reqwest::Client::new(),
      api_base: cfg.api_base.clone().unwrap_or_else(|| DEFAULT_BASE.to_string()),
      api_secret: cfg.api_secret.clone().unwrap_or_default(),
      store_id: cfg.store_id.clone(),
      webhook_secret: cfg.webhook_secret.clone(),
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
    let raw_status = v.get("status").and_then(Value::as_str).unwrap_or("").to_string();
    let amount = v
      .get("amount")
      .and_then(|a| a.get("total"))
      .and_then(Value::as_i64)
      .unwrap_or(0);
    let currency = v.get("currency").and_then(Value::as_str).unwrap_or("KRW").to_string();
    let paid_at = v
      .get("paidAt")
      .and_then(Value::as_str)
      .and_then(|s| DateTime::parse_from_rfc3339(s).ok());

    Ok(PaymentVerification {
      payment_id: v.get("id").and_then(Value::as_str).unwrap_or(payment_id).to_string(),
      transaction_id: v.get("transactionId").and_then(Value::as_str).map(str::to_string),
      status: PaymentStatus::from_portone(&raw_status),
      amount,
      currency,
      paid_at,
      raw_status,
    })
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
    // Response shape: { cancellation: { totalAmount, status, ... } }.
    let cancelled_amount = v
      .get("cancellation")
      .and_then(|c| c.get("totalAmount"))
      .and_then(Value::as_i64)
      .or(amount)
      .unwrap_or(0);
    let raw_status = v
      .get("cancellation")
      .and_then(|c| c.get("status"))
      .and_then(Value::as_str)
      .unwrap_or("CANCELLED")
      .to_string();

    Ok(RefundResult {
      status: if raw_status.contains("PARTIAL") {
        PaymentStatus::PartialCancelled
      } else {
        PaymentStatus::Cancelled
      },
      cancelled_amount,
      raw_status,
    })
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

    Ok(WebhookEvent { event_type, payment_id, transaction_id })
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
