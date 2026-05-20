use serde::Deserialize;

/// SMTP TLS 모드.
///
/// - `None`        — 평문 (테스트/내부 망 전용. 외부 서버 금지)
/// - `Starttls`    — 평문으로 연결 후 STARTTLS로 업그레이드 (587 포트 표준)
/// - `Implicit`    — 초기부터 TLS (465 포트 표준)
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
  /// SMTP 포트. 미지정 시 tls 모드에 따라 기본값 사용
  /// (None=25, Starttls=587, Implicit=465).
  #[serde(default)]
  pub port: Option<u16>,
  /// TLS 모드. 미지정 시 starttls 기본.
  #[serde(default)]
  pub tls: SmtpTlsMode,
}

impl SmtpSettings {
  /// 설정된 포트를 반환하거나, 미지정 시 tls 모드별 표준 포트를 반환.
  pub fn resolved_port(&self) -> u16 {
    self.port.unwrap_or(match self.tls {
      SmtpTlsMode::None => 25,
      SmtpTlsMode::Starttls => 587,
      SmtpTlsMode::Implicit => 465,
    })
  }
}
