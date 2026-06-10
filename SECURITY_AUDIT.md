# Sumida 보안·완성도 점검 보고서

> 최초 작성: 2026-06-10
> 점검 대상: Axum 기반 서버 프레임워크 (5 크레이트 워크스페이스 `api`/`infra`/`entity`/`error`/`e2e`)
> 점검 방식: 인증·세션, ReBAC 인가, 파일·게시판·인프라, 완성도·품질 4개 영역 병렬 감사 + 핵심 항목 코드 직접 재검증

## 사용 방법

- 각 항목을 처리할 때마다 체크박스를 채우고, **진행 기록** 표에 `날짜 / 상태 / 수정 방법 / 커밋` 한 줄을 추가한다.
- 상태값: `미착수` → `진행중` → `완료` / `보류` / `해당없음(오탐)`.
- "✓ 직접 검증" 표시는 보고 시점에 실제 코드로 재확인한 항목.

---

## 전체 평가

설계 골격(ReBAC, DTO 5종 세트, 레이어드 가드, feature 토글)은 견고하고 완성도가 높음(약 85%). 비밀번호 해싱(Argon2id), JWT alg 고정, 로그인 brute-force ban, 페이지네이션 상한·정렬 화이트리스트 등 보안 기본기는 잘 갖춰져 있음. 다만 **무인증 계정 탈취 2건 + 경로 traversal 1건의 Critical**, **full-aws JWT 경로 빌드 결함으로 동작 불가**가 핵심 리스크.

진행 현황 요약: **28 / 28 완료** (Critical 3/3 ✅, High 7/7 ✅, Medium 9/9 ✅, Low·기능 5/5 ✅, 빌드·완성도 4/4 ✅).
- **B-4 DB 통합 테스트**: 실제 Postgres(`sumida_test`)에서 전 스위트 실행·검증 완료 → **ap_util(8), board(9), role(9), organization(9), user(17), dashboard(4) 전부 통과**.
- **M-6 list 가시성**: 의도적 fail-safe 유지(권한 회수된 보드의 자기 글이 목록에서 빠지는 정상 동작). 변경 불필요로 종결.
- (M-2 평문저장·prefix, M-3 access 즉시 폐기, B-2 부팅 진단은 모두 완료)
- **★ 통합 테스트 도중 실제 프로덕션 보안 버그 1건 발견·수정 → [추가발견-1]**: 비-admin 운영자의 user update/delete가 SQL 오류로 **항상 403**(기능 마비). org-ancestors 권한 검사가 생성하는 잘못된 SQL이 원인.

---

## 🔴 Critical — 즉시 수정

### [C-1] 무인증 임의 계정 탈취 (impersonate) ✓ 직접 검증
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/controller/user.rs:142-145`, `api/src/service/user/mod.rs:270-283`
- **근거**: `route_layer(user_guard!)`를 호출한 *뒤* 145행에 `/{user_id}/impersonate`를 등록 → 가드 밖. axum `route_layer`는 호출 시점까지 등록된 라우트에만 적용. 핸들러는 내부 권한 검사 없이 `session.authorize(user_id, ...)`로 세션 승격. 세션 없으면 `from_request_parts`가 미인증 세션 반환 → 누구나 `POST /users/1/impersonate`로 admin 세션 획득.
- **권고**: `impersonate`를 `admin_guard!` 아래로 이동. `change-password`/`activate`/`impersonate`를 별도 Router로 분리해 명시적 가드 부여.

### [C-2] 초대 토큰 user_id 미바인딩 → 임의 계정 비밀번호 재설정
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/service/user/mod.rs:186-230`
- **근거**: `activate`가 body의 `user_id`로 대상을, `UserInvitations::find_by_id(uuid)`로 초대를 각각 조회하고 둘이 같은 초대인지 대조 안 함. 유효한 초대 uuid 보유자가 `{user_id:<admin>, uuid:<자기것>, password:<임의>}`로 임의 계정 비번 덮어쓰기+활성화. C-1과 결합해 무인증.
- **권고**: `invitation.user_id == user_id` 일치 검증 필수화.

### [C-3] 파일 업로드 경로 traversal / 임의 경로 쓰기 ✓ 직접 검증
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/util/storage.rs:192-194`, `service/fs/dto.rs:173-191`, `entity/src/fs/files.rs:109-114`
- **근거**: `resolve_path`가 `base_directory.join(relative)`만 수행 — `PathBuf::join`은 `..` 미정규화, 절대경로 시 base 무시. 저장 디렉터리 `dir`를 클라이언트 multipart `metadata` JSON에서 무검증 수신 → `dir="../../../etc/cron.d"` 또는 `dir="/var/www/html"`로 임의 위치 파일 쓰기(LocalStorage).
- **권고**: canonicalize 후 `starts_with(base_directory)` 검증, `..`·절대경로 거부, `dir` 슬러그화.

---

## 🟠 High

### [H-1] 사용자 생성 시 권한 필드 임의 지정 (백도어 관리자)
- [x] 처리 완료 (2026-06-10)
- **잔여 위험**: 비-admin이 자기 조직 내에서 `role_id`(높은 권한 role)·`is_enabled=true`(즉시 활성)를 지정하는 것은 여전히 가능. role 기반 access_policy 설계에 따라 상승 여지가 있으면 추가 통제 검토.
- **위치**: `api/src/service/user/mod.rs:72-83`, `dto.rs:162-173`
- **근거**: `update`는 비-admin의 `is_admin/role_id/organization_id/is_enabled`를 `NotSet`으로 차단하나 `create`엔 동일 차단 없음 → `{isAdmin:true}`로 관리자 생성.
- **권고**: `create`에도 비-admin 보안 필드 잠금 적용.

### [H-2] access_policy 셀프 발급 (권한 상승)
- [x] 처리 완료 (2026-06-10) — 정책 통제 모델: **"위험 패턴만 차단"**(사용자 결정)
- **남은 위험(설계상 허용)**: 비-admin도 구체 namespace/object/action으로 위임은 가능. 셀프 슈퍼관리자화로 직결되는 `effect=allow` + 와일드카드(`*`)만 차단. 더 엄격한 "권한 위임 검증"이 필요하면 추후 격상.
- **위치**: `api/src/service/access_policy/mod.rs:64-69`, 동일 패턴 `role/mod.rs:66-77`, `api_key/mod.rs`
- **근거**: `create`가 body 정책을 무검증 insert. `access_policy:create` 권한자가 `{namespace:"*",object:"*",action:["*"],effect:"allow",user:"user:<자신>"}`로 슈퍼관리자화. role/api_key의 `access_policies` 주입도 동일.
- **권고**: 생성/수정 정책이 호출자 권한 범위 이내인지 검증. 비-admin은 `user` 필드를 타인/와일드카드로 지정 금지.

### [H-3] 비밀번호 변경 코드 만료·시도제한 없음 (brute-force)
- [x] 처리 완료 (2026-06-10) — 만료 검사 적용. 무차별 대입 창을 3분으로 제한.
- **남은 위험**: 3분 내 고속 무차별 대입은 여전히 이론상 가능(6자리=10^6). 실패 횟수 기반 잠금(스키마에 `attempts` 컬럼)·코드 엔트로피 상향·발송 경로 rate limit은 후속(H-4 / rate-limit 레이어와 연계).
- **위치**: `api/src/service/user_verification/mod.rs:50-56`, `service/user/mod.rs:232-268`
- **근거**: `verify()`가 `expires_at` 미검사 → 6자리 코드 영구 유효. 검증 실패 잠금 없음(`ban_if_exceeded_limit`는 Password 경로 전용). 무인증 경로와 결합해 ~10⁶ brute-force.
- **권고**: `verify`에 `expires_at > now` 필터, 실패 카운트/잠금, 코드 엔트로피 상향.

### [H-4] 검증 발송 엔드포인트 무가드
- [x] 처리 완료 (2026-06-10) — 서비스 계층에서 응답 일원화(열거 차단) + 재발송 쿨다운(폭탄 완화).
- **남은 위험**: IP 기반 전역 rate limit이 없어 user_id를 바꿔가며 다수 대상에 분산 발송하는 폭탄은 가능. rate-limit 레이어(추천 기능 3번)로 보완 필요.
- **위치**: `api/src/controller/user_verification.rs:35-38`
- **근거**: `POST /user-verifications`에 가드 전무 → 임의 user_id로 이메일 폭탄. 응답 차이(`InvalidAccount`/`NoUserEmail`/성공)로 계정 열거. H-3 brute-force 진입점.
- **권고**: 인증/레이트리밋 부여, 응답 일원화.

### [H-5] board_file 목록/조회 ReBAC 미적용·삭제 무검사 (IDOR)
- [x] 처리 완료 (2026-06-10)
- **참고**: 내부 헬퍼 `get_files_for_post`/`get_posts_for_file`은 board_post 상세 흐름에서 호출되며 게시글 가시성은 상위에서 검사되므로 이번 범위에서 제외. 직접 노출 경로가 생기면 ap 적용 필요.
- **위치**: `api/src/service/bps/board_file/mod.rs:42-99, 122-136`
- **근거**: `ap()`(180-184) 정의돼 있으나 `list`/`get`가 `self.select()`를 안 거침 → 전 조직 첨부 열람. `delete`는 `try_access` 없이 `post_id/file_uuid`만으로 원본 파일까지 hard delete.
- **권고**: list/get에 `self.select(...)`, delete에 `self.try_access("delete", ...)` 추가.

### [H-6] FTS 스니펫 구분자 SQL injection
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/util/board.rs:105-128`, `service/bps/board_post/dto.rs:33-43`
- **근거**: `TsHeadlineExpr::apply`가 `format!`로 `ts_headline` 옵션 문자열(`StartSel=...,StopSel=...`)에 쿼리 파라미터 `snippet_start_sel/stop_sel`을 이스케이프 없이 삽입. 작은따옴표로 옵션 리터럴 탈출 → 임의 SQL.
- **권고**: 구분자도 파라미터 바인딩 또는 영숫자·`<>/` 화이트리스트, 최소한 작은따옴표 거부.

### [H-7] 범용 `/files` 업로드 검증 전무
- [x] 처리 완료 (2026-06-10) — storage settings에 전역 업로드 한도 추가 + upload에서 FileValidator 적용.
- **운영 메모**: 새 설정 키 `storage.upload.max-file-size` / `storage.upload.allowed-file-types` 추가됨. 기본값은 크기·타입 무제한(위험 MIME만 차단). 다운스트림 `.env.example`/settings 문서에 반영 필요.
- **위치**: `api/src/service/fs/file.rs:66-110`
- **근거**: `FileValidator` 미호출 → 임의 확장자/MIME/크기. C-3 traversal과 결합 시 위험 증폭.
- **권고**: 범용 업로드에도 전역 `FileValidator`(확장자·MIME·최대 크기) 적용.

---

## 🟡 Medium

### [M-1] JWT/API 키 인증 시 is_enabled·ban 미검사
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/layer/user_guard.rs:93-135`
- **근거**: `verify_jwt`/`verify_api_key`가 `is_enabled`·밴 미검사 → 비활성/차단 계정도 토큰·키만 있으면 통과(세션 경로와 비대칭).
- **권고**: 두 경로에 `is_enabled`·`check_banned` 추가.

### [M-2] API 키 만료 미적용 + 평문 저장
- [x] 만료 미적용: 처리 완료 (2026-06-10) — `verify_api_key`에 `expires_at` 검사 추가.
- [x] 평문 저장: 처리 완료 (2026-06-10) — blake3 해시 저장으로 전환. **무중단**(마이그레이션이 기존 평문을 읽어 해시로 변환).
  - `service/api_key/mod.rs`: `hash_api_key()` 추가, `create()`가 해시 저장 + **평문은 반환값으로 1회만** 노출(정책도 `api:<hash>`에 묶음)
  - `layer/user_guard.rs::verify_api_key`: 입력 키를 해시해서 조회, `profile.api_key`에도 해시 저장
  - `entity/migration/api_key_hash.rs`(신규): 기존 `api_keys.key` + `access_policies."user"`(`api:` 접두)를 해시로 변환. 멱등 가드(이미 해시면 스킵), `down` 불가(단방향). examples/basic·e2e Migrator에 등록.
  - **운영 주의**: 코드 배포 **전에** 이 마이그레이션을 먼저 실행할 것.
- [x] 식별용 key_prefix 완료 (2026-06-10) — `api_keys.key_prefix`(평문 앞 8자, 비밀 아님) 컬럼 추가: entity 필드, init 스키마, `api_key_hash` 마이그레이션(`ADD COLUMN IF NOT EXISTS` + 변환 시 채움), create 저장, list/get DTO 노출. 목록 UI에서 키 식별 가능. entity·api `cargo check` 통과.
- **위치**: `api/src/layer/user_guard.rs:117-122`, `service/api_key/mod.rs:86-96`
- **근거**: `expires_at` 미검사로 만료 키 영구 유효. DB에 평문 저장·평문 비교(`blake3` 의존성 이미 존재하나 미사용).
- **권고**: `expires_at > now` 검사, 키를 해시(blake3/SHA-256)로 저장·비교. (스키마 마이그레이션 동반)

### [M-3] access token 폐기 수단 부재
- [x] refresh 토큰 폐기 인프라 완료 (2026-06-10) — `RedisJwtStore` 실구현(fred 10.x: set/get/del/exists)으로 full-local에서도 refresh 토큰 저장·블랙리스트가 동작(이전 스텁 501 해결). DynamoJwtStore는 기존 구현.
- [x] access token 즉시 폐기 완료 (2026-06-10) — 전역 공유 store(`shared_jwt_store`, `OnceCell<JwtStore>` + 구체 타입 alias로 `dyn` 회피)를 도입하고, `verify_jwt`가 매 인증마다 jti 블랙리스트를 조회. logout이 Authorization 헤더의 access token도 블랙리스트(만료 TTL). full-local·full-aws 양쪽 통과.
- **트레이드오프(주석 명시)**: access token이 stateless→stateful이 되어 매 인증마다 store 조회 비용 발생(특히 DynamoDB는 get_item 비용). 대신 즉시 무효화가 가능.
- **위치**: `api/src/service/auth/helper.rs:149-155`, `token.rs:100-117`
- **근거**: 로그아웃이 refresh만 블랙리스트. `verify_access_token`이 블랙리스트 미조회 → 로그아웃 후에도 access(기본 1h) 유효.
- **권고**: access 검증에 jti 블랙리스트 조회 추가 또는 짧은 TTL 명시.

### [M-4] 세션 고정(session fixation) 방어 부재
- [x] 처리 완료 (2026-06-10) — authorize에 `cycle_id`, logout에 `flush` 적용.
- **남은 위험**: `from_request_parts`가 미인증 요청도 매번 `save()`하는 부분은 remember-me 만료 재적용 로직과 얽혀 있어 이번 범위에서 제외(스토어 bloat은 기능적 이슈, B 영역에서 다룸).
- **위치**: `infra/src/session/mod.rs:206-223, 544-550`
- **근거**: 로그인 시 `cycle_id()` 미호출, 로그아웃이 `flush()` 없이 profile만 default 덮음. `from_request_parts`가 미인증 요청도 매번 `save()`.
- **권고**: 로그인 직후 `cycle_id()`, 로그아웃 시 `flush()`, 미인증 요청 무조건 저장 지양.

### [M-5] 로그인 타이밍 기반 사용자 열거
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/service/auth/session.rs:25-86`, `token.rs:32-95`
- **근거**: 계정 미존재 시 Argon2 검증 없이 즉시 반환(응답 본문 동일, 시간차 노출).
- **권고**: 미존재 계정도 더미 해시 검증으로 시간 균일화.

### [M-6] board_post ap() 객체키/소유권 불일치
- [x] 처리 완료 (2026-06-10) — (A) 객체키 `Id`→`BoardId` 수정, (B) update/delete에 `(board_group AND board) OR 작성자 소유권` 코드 레벨 OR 적용.
- **구조 메모**: `ApOr.push`가 `ApCondition`만 받아 `ApChain` 중첩 불가 → 단건 검사는 코드 레벨 OR로 구현.
- **list 가시성 — 의도적 미변경(fail-safe)**: 작성자가 read 권한 없는 보드의 자기 글을 목록에서 못 볼 수 있는 *과소 노출*이며 **비공개 글 유출 위험은 없음**. read 필터(LATERAL JOIN)에 작성자 OR을 강제하면 fail-open(비공개 글 과다 노출) 위험이 있고 ApOr+ApChain 중첩 재설계가 필요하므로, 위험을 감수하기보다 현 fail-safe 상태를 유지. 필요 시 ap 모듈 확장으로 별도 진행.
- **검증 필요**: 권한 모델 변경이라 `cargo test --package e2e` 권장.
- **위치**: `api/src/service/bps/board_post/mod.rs:527-533 vs 298-345`
- **근거**: 읽기/수정 체인이 `ns("board").column(Id)`로 게시글 id에 매칭(보드 id여야 함). 작성자 소유권 정책(`board_post` ns)이 update/delete 체인에 없어 무력.
- **권고**: "board" 객체 키를 `BoardId`로 수정, `board_post` 소유권을 OR(`ap::or`)로 반영.

### [M-7] 비보안 API 키가 소유자 전체 권한(admin) 상속
- [x] 처리 완료 (2026-06-10) — **현행 유지 + 문서화**(사용자 결정). `verify_api_key`에 의도 주석 추가(비보안=풀 액세스, 보안=api:KEY 격리). 다운스트림 공지 필요.
- **위치**: `api/src/layer/user_guard.rs:111-135`, `util/ap.rs:272-288`
- **근거**: `is_secured=false`면 `api_key=None` → `who()`가 `api:KEY`가 아닌 User/Role/Org/Admin 전체로 펼침.
- **권고**: 의도면 문서화, 키 범위 제한이 목적이면 항상 `api:KEY` 식별자로만 평가.

### [M-8] Content-Disposition 파일명 인젝션
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/util/storage.rs:88`
- **근거**: `attachment; filename="{file_name}"`의 `file_name`(업로드 original_name, 사용자 제어). `"`로 파일명 스푸핑.
- **권고**: RFC 6266 `filename*=UTF-8''` 인코딩, `"`·제어문자 제거.

### [M-9] 멀티파트 핸들러 .unwrap() 패닉 (DoS)
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/service/bps/board_file/dto.rs:114, 144-148`
- **근거**: `field.text().await.unwrap().parse::<i32>().unwrap()` + 필드 누락 시 `.unwrap()` 3연발 → 인증 사용자가 요청 한 번으로 태스크 패닉.
- **권고**: `ok_or(SumidaError::FieldNotProvided)` + `map_err(BadRequest)` (동 파일 metadata 처리 패턴 복사).

---

## 🟢 Low / 기능 결함

### [L-1] CORS `*` + credentials, BanSettings.enabled 무시, JWT 키 길이 미검증, wrap_ilike 와일드카드 미이스케이프, 이미지 디컴프레션 폭탄
- [x] 처리 완료 (2026-06-10) — 5개 모두 적용 + `ban_ip` 인자 순서 버그(B-2 일부)도 동시 수정.
- **위치**: `api/src/layer/cors.rs:11`, `infra/src/settings/security.rs`+`session/mod.rs:440-488`, `auth/helper.rs:37`, `util/db.rs:18`, `util/image.rs:17`+`s3.rs:85`
- **근거/권고**: CORS 설정에 `"*"` 명시 거부 / `ban.enabled` 분기 추가·ban 인자 순서 점검 / 부팅 시 JWT 키 ≥32바이트 검증 / `%`·`_` 이스케이프 / `image::Limits`로 차원·메모리 상한.

### [L-2] 메신저 user room 식별자 불일치 + 무조건 admin 브로드캐스트
- [x] 처리 완료 (2026-06-10)
- **위치**: `infra/src/messenger.rs:57-60 vs 101-105`, `:69`
- **근거**: 접속은 `user:{username}` join, `user()` emit은 `user:{id}` → id 타겟 알림 미도달. 모든 emit에 `.within("admin")` 무조건 포함.
- **권고**: `:104`를 `user_data.id`로 수정, admin 강제 포함 의도면 문서화.

### [L-3] 조회수 증가 로직 역전
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/service/bps/board_post/util.rs:207-239`
- **근거**: 최초/재조회 분기가 뒤바뀌어 DB 갱신이 실제로 일어나지 않음.
- **권고**: 증가 분기 수정 + 테스트.

### [L-4] Pagination ?page=0 → u64 언더플로
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/util/pagination.rs:34`
- **근거**: `(self.page - 1) * self.limit`에서 page=0이면 debug 패닉/release 거대 offset.
- **권고**: `self.page.saturating_sub(1)`.

### [L-5] board_ap! 매크로가 미정의 BoardApLayer 참조
- [x] 처리 완료 (2026-06-10)
- **위치**: `api/src/layer/service_ap.rs:123-131`
- **근거**: `$crate::layer::BoardApLayer` 미정의·미export. 사용처 0이라 컴파일만 통과, 사용 시 실패. 게시판 단위 가드 사실상 미구현.
- **권고**: 레이어 구현 또는 매크로 제거.

---

## ⚙️ 빌드 / 완성도

### [B-1] full-aws JWT 경로 동작 불가 (feature unification 결함) ✓ 컴파일 검증
- [x] 구조 결함 해결 (2026-06-10) — `api/Cargo.toml`의 infra 의존에 `default-features=false` 추가(infra default `session-store-redis` 강제 차단 → full-aws에서 jwt.rs cfg_if가 dynamodb 분기 선택), 루트 `Cargo.toml`의 `e2e`를 dev-dependency로 격리(full-local 오염 제거). **검증**: `cargo check -p api --no-default-features --features full-aws`(이전 E0432 실패 → 통과, DynamoJwtStore 선택), `cargo check -p sumida --features full-aws` 통과, full-local 회귀 없음.
- [x] **RedisJwtStore 실구현 완료** (2026-06-10, M-3에서 처리) — fred 10.x로 구현해 full-local `/auth/token`이 동작(501 해결). `cargo check -p infra --features session-store-redis,jwt` 통과.
- **참고**: `examples/basic`의 `create_dummy_board_posts` 바이너리는 구 엔티티 스키마 참조로 깨진 **기존 stale 에러**(TASKS P2). 본 변경과 무관.
- **위치**: `api/Cargo.toml`(infra 의존에 default-features 미차단), 루트 `Cargo.toml`(e2e 일반 의존), `infra/src/jwt.rs:44`, `service/auth/token.rs:180-194`
- **근거**: api→infra에 `default-features=false` 없어 infra default(`session-store-redis`) 항상 on, `jwt.rs` cfg_if가 redis 분기만 컴파일 → `DynamoJwtStore`는 어떤 조합에서도 미선택 dead code. `RedisJwtStore`는 전 메서드 `Unsupported` 스텁 → 모든 빌드에서 `/auth/token` 501. TASKS.md의 "DynamoJwtStore production-ready"와 불일치.
- **권고**: 워크스페이스 내부 의존성 전부 `default-features=false`, 루트 `e2e`를 dev-dependency 강등, mutually-exclusive feature는 `compile_error!` 가드.

### [B-2] 핸들러 경로 런타임 패닉 다수
- [x] 핸들러 경로 완료 (2026-06-10) — redis fallback URL `http://`→`redis://` + `?` 전파(`redis_store.rs`), messenger `of().unwrap()`→조용한 무시(`messenger.rs`), board_post update/delete TOCTOU unwrap→`ok_or_else`(M-6에서 처리).
- [x] 부팅 경로 완료 (2026-06-10) — `settings/mod.rs`(설정 로딩 실패), `tracing.rs`(잘못된 logging 필터), `db.rs`(잘못된 logging 레벨)의 unwrap을 진단 메시지가 있는 `expect`/`unwrap_or_else(panic!)`로 교체. infra `cargo check` 통과.
- **위치**: `board_post/mod.rs:357`(TOCTOU unwrap), `session/redis_store.rs:17`(`http://` fallback URL + unwrap), `settings/mod.rs:51`(lazy_static unwrap), `tracing.rs:13`·`db.rs:53`(from_str unwrap), `messenger.rs:67`(of unwrap), `aws.rs:23`
- **근거/권고**: 핸들러·런타임 경로 unwrap을 `?`/`ok_or`로 전환, Redis fallback 스킴 `redis://`로 수정·상수 단일화, 설정 실패 진단 메시지.

### [B-3] 에러 코드 체계 미구현
- [x] 처리 완료 (2026-06-10) — `error.rs::code()`에 variant별 카테고리 코드(1xxx 인증, 2xxx 데이터, 3xxx 메일, 4xxx 파일, 5xxx 런타임, 6xxx 요청, 7xxx 외부, 9xxx 미분류) 부여. `cargo check -p error --features full` 통과.
- **위치**: `error/src/error.rs:126-135`
- **근거**: `code()`가 `NoContent=0` 외 전부 `E-1` → release(reason 가림)에서 클라이언트가 에러 종류 구분 불가.
- **권고**: variant별 코드 부여(다운스트림 `SumidaApiError::error_code()` 훅 호환).

### [B-4] 테스트 공백 (보안 경계)
- [x] 핵심 단위 테스트 추가 (2026-06-10) — DB 불필요한 순수 보안 로직 11개 테스트 신규(모두 통과):
  - `util/ap.rs`: `assert_policy_within_authority` 7건(H-2: admin 우회, deny 와일드카드 허용, 구체 allow 통과, ns/object/action/user 와일드카드 allow 거부)
  - `util/storage.rs`: `percent_encode_rfc5987` 3건(M-8: attr-char 보존, 공백/따옴표/UTF-8 인코딩)
  - `service/api_key/mod.rs`: `hash_api_key` 1건(M-2: 결정성·hex64·충돌없음)
- [ ] **DB 통합 테스트 잔여(환경 제약)**: layer 인증 가드·ap LATERAL JOIN SQL 등 DB 기반 테스트는 자격증명이 설정된 Postgres 테스트 DB가 필요. 현 환경은 Postgres 18이 떠 있으나 인증 자격증명 미설정으로 실행/검증 불가 → 작성보다 실행 검증이 핵심이라 별도 환경에서 진행. 기존 `monitoring` 단위테스트는 본 작업과 무관하게 환경 의존으로 실패(별도 수정 대상).
- **근거**: 모든 `layer/`(user_guard/admin_guard/service_ap/board_ap), auth session/token(rotation·blacklist), api_key, ban, mail_form, user_invitation/verification, fs/file, messenger, error 매핑에 단위 테스트 0. ap 코어는 통합 테스트(ap_util)만.
- **권고**: 보안 경계 레이어부터 단위/통합 테스트 추가.

---

## 💡 추가 권장 기능 (로드맵)

우선순위순. 이미 존재하는 것(graceful shutdown 예제, access_logs 감사, 로그인 ban, 모니터링 SSE, 메일, 세션/JWT)은 제외.

- [ ] 1. **표준 `/health` + `/ready`** — `Sumida::router()`가 DB·세션 스토어 ping 포함 제공 (비용 대비 효과 최고)
- [ ] 2. **요청 ID + TraceLayer 기본 내장(opt-out)** — tower-http 이미 의존, access_logs 연결
- [ ] 3. **Rate limiting 레이어** — `/auth/*` 보호, `Busy→429` 매핑 이미 준비됨
- [ ] 4. **OpenAPI 생성(utoipa)** — DTO 5종 세트 정형화로 derive 비용 낮음, 클라이언트 타입 자동화
- [ ] 5. **경량 백그라운드 잡 러너** — `cleanup_expired` 호출처 0곳, 로그·밴 보존정책 부재
- [ ] 6. **메트릭 export(Prometheus)** — 모니터링 SSE provider를 `/metrics`로 노출
- [ ] 7. **OIDC 1개 공급자(사내 SSO)** — `verify_jwt`/`authorize()` 재사용
- [ ] 8. **2FA/TOTP** — `user_verifications` 인프라 재사용, admin 한정 강제 옵션
- [ ] 9. **웹훅 디스패처** — 서버-대-서버 알림(재시도·서명)
- [ ] 10. **업로드 파일 스캔 훅 + 멱등성 키** — ClamAV 훅 포인트(트레잇)

---

## 진행 기록

| 날짜 | 항목 | 상태 | 수정 방법 | 커밋 |
|---|---|---|---|---|
| 2026-06-10 | (전체) | 보고서 작성 | 4개 영역 병렬 감사 + 핵심 항목 코드 재검증 | — |
| 2026-06-10 | C-1 | 완료 | ① `controller/user.rs` 라우터에서 `impersonate`를 `user_guard` 보호 그룹으로 이동하고 `change-password`/`activate`는 pre-auth 라우트로 명시 분리(`route_layer` 뒤 등록 버그 제거) ② `service/user/mod.rs::impersonate` 진입부에 `is_admin` 검사 추가(미관리자 `OnlyAdminAllowed`). `cargo check -p api --features full-local` 통과 | (미커밋) |
| 2026-06-10 | C-2 | 완료 | `service/user/mod.rs::activate`의 초대 조회에 `Column::UserId.eq(user_id)` 필터 추가 → uuid와 user_id 바인딩. 불일치 시 `NoData`(404). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | C-3 | 완료 | `util/storage.rs::LocalStorage::resolve_path`를 컴포넌트 검증 방식(`Normal`/`CurDir`만 허용, `..`·절대경로 `BadRequest` 거부)으로 변경하고 `-> Result` 시그니처로 전환. `store`/`delete`/`stream`/`get_file_size`에서 `?` 전파, `get_uri`는 실패 시 빈 문자열. `cargo check --features full-local` 통과 | (미커밋) |
| 2026-06-10 | H-1 | 완료 | `service/user/mod.rs::create`에 비-admin 분기 추가: `is_admin=Set(false)`, `organization_id=Set(세션 조직 id)` 강제. role_id/is_enabled는 NOT NULL·정상 입력이라 유지(잔여 위험 기록). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | H-2 | 완료 | `util/ap.rs`에 `assert_policy_within_authority`(비-admin은 effect=allow+와일드카드 namespace/object/action/user 거부) 추가. access_policy create/update(병합 후 검증), role create/update, api_key create/update의 정책 주입 4경로에 적용. 모델="위험 패턴만 차단"(사용자 결정). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | H-3 | 완료 | `service/user_verification/mod.rs::verify`에 `Column::ExpiresAt.gt(Utc::now())` 필터 추가 → 만료 코드 거부, 무차별 대입 창 3분 제한. 시도 제한은 잔여 위험 분리. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | H-4 | 완료 | `service/user_verification/mod.rs::send` 재작성: user 조회를 앞으로 옮기고 user/email 부재 시 `Ok(())` 조용히 성공(열거 차단), 유효 코드 존재 시 재발송 스킵(3분 쿨다운). IP rate limit은 잔여 위험. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | H-5 | 완료 | `service/bps/board_file/mod.rs`: list/get을 `self.select(BoardFiles::find())`로 감싸 read ReBAC 적용, delete에 `try_access("delete")` + 트랜잭션 추가. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | H-6 | 완료 | `util/board.rs::TsHeadlineExpr::apply`: ts_headline 옵션 문자열을 Rust에서 조립 후 4번째 인자로 `$2` 파라미터 바인딩 → start_sel/stop_sel 직접 보간 제거(SQLi 차단). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | H-7 | 완료 | `infra/settings/storage.rs`에 `UploadSettings`(max_file_size/allowed_file_types) 추가, `service/fs/file.rs::upload`에서 `FileValidator`로 size/extension/MIME 검증. 기본값 무제한+위험 MIME 차단. infra·api `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-1 | 완료 | `layer/user_guard.rs` verify_jwt/verify_api_key에 `is_enabled` + `check_banned` 검사 추가. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-2 | 부분완료 | `verify_api_key`에 `expires_at < now` 검사 추가(만료 적용). 평문 저장 해시화는 breaking이라 후속 분리. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-5 | 완료 | `service/auth/helper.rs`에 `dummy_verify_password`(OnceLock 캐시 더미 해시) 추가, session.rs/token.rs login의 계정부재 분기에서 호출 → argon2 비용 균일화. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-4 | 완료 | `infra/session/mod.rs`: authorize에 `cycle_id`(세션 고정 방어), logout에 `flush`(완전 파기), identify 내부 정리는 profile reset 인라인으로 분리. infra·api `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-8 | 완료 | `util/storage.rs::download`: Content-Disposition을 ASCII fallback + `filename*=UTF-8''`(RFC 5987) 조합으로 변경, `percent_encode_rfc5987` 헬퍼 추가(의존성 무). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-9 | 완료 | `service/bps/board_file/dto.rs::from_multipart`: board_id text/parse unwrap → `map_err(BadRequest)`, 최종 필드 3개 unwrap → `ok_or_else(BadRequest)`. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-6 | 완료(부분잔여) | `board_post/mod.rs`: ap() 객체키 `Id`→`BoardId`(A), `authorize_post_mutation` helper로 update/delete에 운영자 OR 작성자 소유권 검사(B), update의 TOCTOU unwrap도 `ok_or_else`로 수정. list 가시성 OR은 잔여. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-7 | 완료 | `layer/user_guard.rs::verify_api_key`에 키 범위 정책 의도 주석 추가(현행 유지 결정). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | L-1 | 완료 | CORS `*` 필터+경고(`cors.rs`), `wrap_ilike` LIKE 이스케이프(`db.rs`), JWT 키 ≥32바이트 검증(`helper.rs`), `ban.enabled` 체크 + `ban_ip` 인자순서 수정(`session/mod.rs`), 이미지 `into_response` expect 제거 + `get_image` decode limits(`image.rs`/`s3.rs`). infra·api `cargo check` 통과 | (미커밋) |
| 2026-06-10 | L-2 | 완료 | `messenger.rs`: user 룸 join을 username→id로 수정(타겟 알림 도달), 무조건 admin 브로드캐스트는 의도 주석 명시. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | L-3 | 완료 | `board_post/util.rs::increase_view_count_if_not_viewed`: 조건 역전 수정(최초 조회 시 +1). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | L-4 | 완료 | `pagination.rs::paginate`: `(page-1)` → `page.saturating_sub(1)`로 u64 언더플로 방지. `cargo check` 통과 | (미커밋) |
| 2026-06-10 | L-5 | 완료 | `layer/service_ap.rs`: 미정의 `BoardApLayer` 참조하던 깨진 미사용 `board_ap!` 매크로 제거(설명 주석 대체). `cargo check` 통과 | (미커밋) |
| 2026-06-10 | B-2 | 핸들러완료 | redis fallback URL `redis://` + `?`(`redis_store.rs`), messenger `of()` 패닉→조용한 무시(`messenger.rs`), board_post TOCTOU(M-6). 부팅 경로 unwrap은 잔여. infra `cargo check` 통과 | (미커밋) |
| 2026-06-10 | B-3 | 완료 | `error.rs::code()`에 variant별 카테고리 코드 부여(1xxx~9xxx). `cargo check -p error --features full` 통과 | (미커밋) |
| 2026-06-10 | B-1 | 구조완료 | `api/Cargo.toml` infra 의존 `default-features=false`, 루트 `Cargo.toml` e2e를 dev-dependency로. full-aws에서 DynamoJwtStore 선택 확인(`-p api --no-default-features --features full-aws` 통과). RedisJwtStore 실구현은 별도 잔여 | (미커밋) |
| 2026-06-10 | M-2(평문) | 완료 | `service/api_key`: blake3 해시 저장, 평문 1회 반환. `verify_api_key` 해시 조회. `entity/migration/api_key_hash.rs` 신규(평문→해시 무중단 변환, access_policies api: 접두 포함), Migrator 등록. api/entity/basic `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-3 | 부분완료 | `infra/jwt.rs` RedisJwtStore fred 10.x 실구현(refresh 저장·블랙리스트·exists). full-local `/auth/token` 501 해결. access token verify 통합은 구조변경 필요로 잔여. infra/api `cargo check` 통과 | (미커밋) |
| 2026-06-10 | B-4 | 부분완료 | ap/storage/api_key에 순수 보안 로직 단위 테스트 11건 추가, 전부 통과(`cargo test -p api --lib tests::`). DB 통합 테스트는 자격증명 미설정으로 잔여 | (미커밋) |
| 2026-06-10 | M-3(access) | 완료 | `token.rs` 전역 `shared_jwt_store`(OnceCell+구체타입 alias), `verify_jwt`에 jti 블랙리스트 조회, `logout`이 Authorization 헤더 access token도 블랙리스트. full-local·full-aws 통과 | (미커밋) |
| 2026-06-10 | B-2(부팅) | 완료 | settings/tracing/db의 부팅 unwrap을 진단 메시지 expect/panic으로 교체. infra `cargo check` 통과 | (미커밋) |
| 2026-06-10 | M-2(prefix) | 완료 | `api_keys.key_prefix` 컬럼(평문 앞8자) 추가: entity/init/마이그레이션/create/DTO. 목록 식별용. entity·api `cargo check` 통과 | (미커밋) |


| 2026-06-10 | B-4(통합) | 완료 | `sumida_test` Postgres에서 통합 테스트 실행: ap_util 8/8, board 9/9, role 9/9, organization 9/9 통과. user 14/17(실패 3건은 기존 불일치, 내 변경 무관 — stash로 확정). 부수 수정: server.rs 동적 포트, ap_test_utils `create_access_policy` 별칭, settings.e2e.toml 생성 | (미커밋) |
| 2026-06-10 | create_dummy | 완료 | 구 스키마 참조 stale 에러 복구(organizations/roles 없는 필드 제거, topic_id Option화, content_type→BoardPostContentType::Html, .iter()→into_iter(), comments_count). `cargo check --workspace` 전체 통과 | (미커밋) |
| 2026-06-10 | 추가발견-1 | 완료 | **프로덕션 보안버그**: org-ancestors 권한검사가 `col=(ANY(...))` 잘못된 SQL 생성 → 비-admin user update/delete 항상 403. `org.rs` 완성 비교식 반환 + `ap.rs` in_subquery 직접 OR 결합으로 수정. DB 테스트로 403→200 검증 | (미커밋) |
| 2026-06-10 | 추가발견-2 | 완료 | dashboard 테스트 `bytes_stream`용 reqwest `stream` feature 추가. dashboard 4/4 통과 | (미커밋) |
| 2026-06-10 | 테스트인프라 | 완료 | server.rs ephemeral 포트, ap_test_utils create_access_policy 별칭, user.rs create_in_org + update/wildcard 테스트 재작성, settings.e2e/.env.e2e example 추가 | (미커밋) |

---

## 통합 테스트로 검증된 보안 변경

`sumida_test` DB(실 Postgres)에서 실행해 다음이 실제 동작함을 확인:
- **ReBAC 코어**(ap_util): LATERAL JOIN 필터, `who()` 세션 확장, allow/deny effect, 와일드카드, EditAp 저장/갱신/삭제 — 8/8
- **board 권한**(H-5 인접 영역): service_ap, admin 우회, explicit deny, 와일드카드 action/object — 9/9
- **role 권한**(H-2 직접): role create/update 시 정책 주입 + `assert_policy_within_authority` 경로 포함 — 9/9
- **organization 권한** — 9/9
- **api_key_hash 마이그레이션**(M-2): 적용 성공 + 재적용 멱등성(`key_prefix already exists, skipping`) 확인

## 통합 테스트 중 발견한 추가 버그 (모두 수정 완료)

### [추가발견-1] ★ 프로덕션 보안 버그: org-scoped 권한 검사 SQL 오류로 user update/delete가 항상 403
- [x] 수정 완료 (2026-06-10)
- **심각도**: High (인가 기능 마비). 비-admin 운영자가 `org:user` 권한을 정상 보유해도 user update/delete가 **무조건 403**. 즉 "조직 단위 user 관리" 기능이 admin 외에는 전혀 동작하지 않았음.
- **근본 원인**: `util/org.rs::organization_ancestors_subquery`가 `ANY(...)` 표현식을 반환하고, `util/ap.rs::_build_object_condition`이 이를 `col.eq(expr)`로 감쌌다. sea-query가 이를 `"ap"."object" = (ANY(...))`로 렌더링 → **PostgreSQL 문법 오류**. `can_access`는 쿼리 에러를 `.ok()`로 삼켜 `false`(=거부) 반환. 또한 raw string 전환 시 `\` 줄잇기 문자가 리터럴로 남아 SQL이 깨지는 2차 문제도 있었음.
- **수정**: `organization_ancestors_subquery`가 `"ap"."object" = ANY(...)` **완성 비교식**을 반환하도록 변경(개행 정리 포함), `_build_object_condition`은 in_subquery를 `col.eq()`로 감싸지 않고 `cond.add(expr)`로 직접 OR 결합.
- **검증**: 실제 DB에서 `test_user_update_permissions_comprehensive` 등 3건이 403→200으로 통과. user 17/17, ap_util 8/8 회귀 없음.
- **발견 경위**: 처음엔 "테스트 불일치(기존버그-1)"로 추정했으나, 조건 분리 디버깅(`object_in_subquery`만 추가하면 `can_access=false`) + 생성 SQL 덤프로 프로덕션 코드 버그임을 확정.

### [추가발견-2] dashboard 테스트 컴파일 불가 (`bytes_stream` 메서드 없음)
- [x] 수정 완료 (2026-06-10) — `e2e/Cargo.toml`의 reqwest features에 `"stream"` 추가. dashboard 4/4 통과.

## 테스트 인프라 수정 (B-4 검증을 위해 필요했음)
- `e2e/src/server.rs`: 고정 포트(`server.host`) → ephemeral 포트(`host:0`) 바인딩. 동시 실행 시 포트 충돌(Address already in use) 해소.
- `e2e/src/ap_test_utils.rs`: 옛 테스트가 참조하는 `create_access_policy`를 `create_ap`의 별칭으로 추가(컴파일 복구).
- `e2e/src/user.rs`: `TestUser::create_in_org`(기존 조직에 user 생성) 추가 — org-scoped 권한 테스트용.
- `api/tests/user.rs`: update/wildcard 테스트 3건을 user의 실제 권한 모델(`user` service + `org:user` row guard, same-org target)에 맞게 재작성.
- `settings.e2e.toml` / `.env.e2e`는 자격증명 포함이라 gitignore 유지. 대신 `settings.e2e.example.toml`, `.env.e2e.example` 추가.
