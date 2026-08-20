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

// 종전에는 여기에 `ApiService<A: ApGuard, E>` 트레이트가 있었다. 서비스마다 `ap()` 로 조건을
// 세우고 `select` / `try_access` 를 기본 구현으로 받아 가던 자리이며, 그 기본 구현이 곧 레거시
// 평가기 호출이었다. 인가가 컴파일된 규칙으로 옮겨가면서 구현체가 전부 걷혔고(sumida 17→0),
// 각 서비스가 `Authz::filter` / `Authz::try_check` 를 자기 `select` / `ensure` 안에서 직접
// 부르게 되면서 공유할 기본 구현이 남지 않아 트레이트도 함께 사라졌다.
//
// 트레이트를 되살리는 대신 서비스마다 고유 메서드를 두는 편이 옳다: 정의 이름과 퍼미션 이름은
// 서비스마다 다르고, 그 둘을 타입 매개변수로 밀어 올려 봐야 호출부에서 다시 문자열로 내려온다.
