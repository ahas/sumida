pub mod access_log;
pub mod api_key;
pub mod auth;
pub mod ban;
#[cfg(feature = "board")]
pub mod bps;
pub mod dashboard;
#[cfg(feature = "file")]
pub mod fs;
pub mod grants;
#[cfg(feature = "mail-form")]
pub mod mail_form;
pub mod me;
#[cfg(feature = "membership")]
pub mod membership;
pub mod monitoring;
pub mod organization;
pub mod relation_tuple;
pub mod role;
#[cfg(feature = "system-settings")]
pub mod system_settings;
pub mod user;
pub mod user_invitation;
pub mod user_verification;

pub mod prelude;

// 인가를 공통 트레이트의 기본 구현으로 내려 주지 않는다. 각 서비스가 자기 `select` / `ensure`
// 안에서 `Authz::filter` / `Authz::try_check` 를 직접 부른다.
//
// 정의 이름과 퍼미션 이름은 서비스마다 다르므로 공유할 기본 구현이 없다. 둘을 타입 매개변수로
// 밀어 올려 봐야 호출부에서 다시 문자열로 내려오고, 그 사이에 인가를 한 겹 감싼 층이 하나 더
// 생긴다.
