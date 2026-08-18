//! 컴파일타임 능력(capability) 레지스트리.
//!
//! 각 기능 모듈은 자신의 feature가 켜졌을 때만 `inventory::submit!`으로 자기 자신을 등록한다.
//! 따라서 `inventory::iter::<Capability>`는 런타임에 "이 빌드에 켜진 기능 집합"을 정확히 돌려준다.
//! sumida 를 쓰는 서비스도 `sumida::api::Capability`를 동일하게 submit 하면 링크타임에 병합된다.
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Capability {
  /// 안정적 키(와이어/클라 식별자). 예: "board", "cloud".
  pub key: &'static str,
  /// 소유 그룹. 예: "sumida", 또는 소비 서비스의 이름.
  pub group: &'static str,
  /// 사람이 읽는 라벨.
  pub label: &'static str,
}

inventory::collect!(Capability);

/// 이 빌드에 컴파일된(=켜진) 모든 능력.
pub fn enabled() -> Vec<&'static Capability> {
  inventory::iter::<Capability>.into_iter().collect()
}

/// 특정 키가 켜져 있는지.
pub fn has(key: &str) -> bool {
  inventory::iter::<Capability>.into_iter().any(|c| c.key == key)
}

// --- sumida 자체 기능 능력(각 feature가 켜졌을 때만 등록) ---
#[cfg(feature = "board")]
inventory::submit!(Capability { key: "board", group: "sumida", label: "Board System" });
#[cfg(feature = "file")]
inventory::submit!(Capability { key: "file", group: "sumida", label: "File System" });
#[cfg(feature = "membership")]
inventory::submit!(Capability { key: "membership", group: "sumida", label: "Membership / Billing" });
#[cfg(feature = "mail-form")]
inventory::submit!(Capability { key: "mail-form", group: "sumida", label: "Mail Form" });
