use serde::Deserialize;

/// SMTP TLS mode.
///
/// - `None`        — plaintext (test/internal network only. Not for external servers)
/// - `Starttls`    — connect in plaintext then upgrade with STARTTLS (standard for port 587)
/// - `Implicit`    — TLS from the start (standard for port 465)
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SmtpTlsMode {
  None,
  #[default]
  Starttls,
  Implicit,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SmtpSettings {
  pub username: String,
  pub password: String,
  pub server: String,
  pub auth: String,
  /// SMTP port. When unspecified, uses a default based on the tls mode
  /// (None=25, Starttls=587, Implicit=465).
  #[serde(default)]
  pub port: Option<u16>,
  /// TLS mode. Defaults to starttls when unspecified.
  #[serde(default)]
  pub tls: SmtpTlsMode,
}

impl SmtpSettings {
  /// Returns the configured port, or the standard port for the tls mode when unspecified.
  pub fn resolved_port(&self) -> u16 {
    self.port.unwrap_or(match self.tls {
      SmtpTlsMode::None => 25,
      SmtpTlsMode::Starttls => 587,
      SmtpTlsMode::Implicit => 465,
    })
  }
}
