//! 문서 파생 매크로가 읽는 타입 별칭을 남긴다.
//!
//! 파생은 필드 타입을 **문법**으로만 읽는다. `DateTimeWithTimeZone` 은 sea-orm 이 chrono 타입에
//! 붙인 이름이라, 그 사실을 여기서 한 번 말해 주지 않으면 시각 필드마다 "string, date-time" 을
//! 손으로 적어야 하고 그 손글씨가 필드 타입과 따로 놀 수 있다.
//!
//! 문서를 만들지 않는 빌드에서는 아무것도 하지 않는다.
fn main() {
  #[cfg(feature = "openapi")]
  utoipa_config::Config::new()
    .alias_for("DateTimeWithTimeZone", "chrono::DateTime<chrono::FixedOffset>")
    .write_to_file();
}
