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
  pub provider: String,
  /// API base override (PortOne default: https://api.portone.io).
  #[serde(default)]
  pub api_base: Option<String>,
  /// PortOne V2 API secret (used to obtain a Bearer access token).
  #[serde(default)]
  pub api_secret: Option<String>,
  /// Optional PortOne store id.
  #[serde(default)]
  pub store_id: Option<String>,
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

fn default_provider() -> String {
  "none".to_string()
}

impl Default for PaymentSettings {
  fn default() -> Self {
    Self {
      provider: default_provider(),
      api_base: None,
      api_secret: None,
      store_id: None,
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
      payment: PaymentSettings::default(),
    }
  }
}
