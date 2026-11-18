use serde::Deserialize;

/// Default order in which the entitlement gate tries to satisfy a guarded action
/// when an owner has both an active subscription and a credit balance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateOrder {
  /// Try the active subscription first, fall back to consuming credits.
  #[default]
  SubscriptionFirst,
  /// Consume credits first, fall back to the active subscription.
  CreditFirst,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct MembershipSettings {
  /// ISO 4217 currency used when a plan / wallet does not specify one.
  #[serde(default = "default_currency")]
  pub default_currency: String,
  /// Default subscription-vs-credit gate ordering.
  #[serde(default)]
  pub default_gate_order: GateOrder,
  /// Default expiry (in days) applied to granted credit lots. `None` = never expire.
  #[serde(default)]
  pub credit_default_expiry_days: Option<i32>,
  /// How many days before `current_period_end` an "expiring soon" notice is sent.
  #[serde(default = "default_expiring_soon_notify_days")]
  pub expiring_soon_notify_days: i32,
  /// Balance at or below which a "low balance" alert is sent. `None` = disabled.
  #[serde(default)]
  pub credit_low_balance_threshold: Option<i64>,
  /// Days a subscription stays entitled after a failed renewal before final
  /// expiry (dunning grace window). 0 = no grace (immediate loss of entitlement).
  #[serde(default)]
  pub grace_days: i32,
  /// Max failed renewal-charge retries during the grace window before giving up.
  /// 0 = no retries (the subscription simply expires at grace end).
  #[serde(default)]
  pub max_dunning_attempts: i32,
  /// Sales tax applied at checkout, in basis points (e.g. 1000 = 10%). 0 = none.
  #[serde(default)]
  pub tax_percent_bp: i32,
  /// Hours after which an unconfirmed pending purchase is expired (and its coupon
  /// redemptions reversed). 0 = disabled (never auto-expire).
  #[serde(default)]
  pub pending_purchase_ttl_hours: i32,
  /// Lifetime, in seconds, of a wallet or quota hold whose caller names none. An extension
  /// restarts it. Every hold expires, so one a caller walked away from never strands its amount.
  #[serde(default = "default_reservation_ttl_secs")]
  pub reservation_ttl_secs: i64,
  /// Payment-gateway (PG) configuration.
  #[serde(default)]
  pub payment: PaymentSettings,
}

/// Payment-gateway configuration. `provider` selects the implementation; PortOne
/// is the first-class one, but the abstraction (`infra::payment::PaymentProvider`)
/// accommodates additional PGs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub struct PaymentSettings {
  /// "none" (disabled, default) | "portone" | "mock" (tests).
  #[serde(default = "default_provider")]
  pub provider:       String,
  /// API base override (PortOne default: https://api.portone.io).
  #[serde(default)]
  pub api_base:       Option<String>,
  /// PortOne V2 API secret (used to obtain a Bearer access token).
  #[serde(default)]
  pub api_secret:     Option<String>,
  /// Optional PortOne store id.
  #[serde(default)]
  pub store_id:       Option<String>,
  /// Optional PortOne channel key, used for billing-key (recurring) payments when
  /// the store has multiple channels.
  #[serde(default)]
  pub channel_key:    Option<String>,
  /// Webhook signing secret (Standard Webhooks, e.g. `whsec_...`).
  #[serde(default)]
  pub webhook_secret: Option<String>,
}

fn default_currency() -> String {
  "KRW".to_string()
}

fn default_expiring_soon_notify_days() -> i32 {
  7
}

fn default_reservation_ttl_secs() -> i64 {
  3600
}

fn default_provider() -> String {
  "none".to_string()
}

impl Default for PaymentSettings {
  fn default() -> Self {
    Self {
      provider:       default_provider(),
      api_base:       None,
      api_secret:     None,
      store_id:       None,
      channel_key:    None,
      webhook_secret: None,
    }
  }
}

impl Default for MembershipSettings {
  fn default() -> Self {
    Self {
      default_currency: default_currency(),
      default_gate_order: GateOrder::default(),
      credit_default_expiry_days: None,
      expiring_soon_notify_days: default_expiring_soon_notify_days(),
      credit_low_balance_threshold: None,
      grace_days: 0,
      max_dunning_attempts: 0,
      tax_percent_bp: 0,
      pending_purchase_ttl_hours: 0,
      reservation_ttl_secs: default_reservation_ttl_secs(),
      payment: PaymentSettings::default(),
    }
  }
}
