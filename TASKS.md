# Tasks / Roadmap

다른 PC에서도 동일하게 확인할 수 있는 후속 작업 목록. 마치면 항목을 제거하거나
별도 "완료" 섹션으로 옮긴다. 작업 단위가 크면 별도 TASKS_*.md 로 분리할 수 있다.

## P1 — 운영 직결

### G5c. RedisJwtStore 실구현
- 위치: `infra/src/jwt.rs:53-112`
- 현재 상태: 모든 메서드가 `SumidaError::Unsupported("Redis JWT store not yet implemented")` 반환.
  따라서 `session-store-redis` feature로 빌드하면 `/auth/token` 호출 시 501.
- 필요 작업
  - `fred` crate(`Cargo.toml`에 이미 의존성 있음)로 RedisJwtStore::new에서 연결 풀 생성
  - `store_refresh_token` / `get_refresh_token` / `delete_refresh_token` — 키 `refresh_token:{jti}` + TTL = `expires_at`
  - `blacklist_token` / `is_token_blacklisted` — 키 `blacklist:{jti}` + TTL
  - `cleanup_expired` — TTL 자동 만료에 의존(noop)으로 두거나 명시적 SCAN
- 검증
  - `examples/basic`을 full-local로 띄우고 user 시드 후 `POST /auth/token`로 happy-path 확인
  - refresh rotation: 새 refresh로 또 refresh했을 때 이전 jti가 무효화되는지 확인

### G5d. DynamoDB Local 기반 /auth/token e2e 테스트
- `DynamoJwtStore`(`infra/src/jwt.rs:117-262`)는 production-ready이지만 자동 검증이 없음
- `e2e/` 또는 `examples/basic/tests/`에 DynamoDB Local(또는 LocalStack) 기반 시나리오 추가
- 시나리오: login → refresh → 두 번째 refresh 시 첫 refresh 무효 확인 → logout → blacklist 적용 확인

### Access token 블랙리스트
- 현재는 refresh token만 logout 시 블랙리스트(`service/auth/token.rs::logout`)
- access token이 짧으니 봐줄 만하지만, 즉시 무효화가 필요한 경우(보안 사고, 권한 회수)에 부족
- 옵션 A: helper.rs에 `revoke_access_token(jti, expires_at)` + 검증 시 `is_token_blacklisted` 호출
- 옵션 B: access TTL을 더 짧게(예: 5분)로 줄이고 그대로 두기

## BPS — 게시판 검토 후속 (2026-06 코드리뷰)

`bs` 브랜치 게시판(`bps`) 도메인 리뷰 결과. 우선순위 P0(배포 차단) → P2.

### P0-1. `board_posts.user_id` FK가 `boards`를 가리킴 ⚠️
- 위치: `entity/src/migration/board_system.rs:294-301`
- `fk_board_posts__user_id`가 `BoardPosts::UserId → Boards::Id`로 연결됨. 글 생성 시 `user_id`에
  로그인 유저 id가 들어가는데(`board_post/mod.rs:322`), DB FK는 그 값이 `boards.id`에 있어야 통과 →
  유저/보드 id 안 겹치면 **모든 글 작성이 FK 위반으로 실패**.
- 작업: FK 대상 테이블을 `users`로 수정 + on_delete `SetNull`(작성자 삭제 시 글 보존) 검토.

### P0-2. `get_articles`가 `Notice`로 필터링
- 위치: `api/src/service/bps/board_post/mod.rs:117`
- `kind=article` 기본 분기(`board.rs:43`)가 공지만 반환. `BoardPostKind::Notice` → `Article`로 수정.

### P0-3. `GET /board-posts/{id}`가 비밀번호·가시성 우회 ⚠️
- 위치: `board_post/mod.rs:196-216`(get) vs `218-296`(view)
- `view()`는 비밀번호 검증 + `check_post_visible`(멤버/비공개) 수행하나 `get()`은 둘 다 생략하고 본문 반환.
- 작업: `get()`에도 동일 가드 적용해 두 읽기 경로 통일.

### P0-4. 반응(reaction) 권한/소유자/ap 깨짐
- 위치: `board_post_reaction/mod.rs:65-76`(create), `113-122`(ap 재사용)
- create: 권한 체크 없음 + `user_id`를 클라이언트 입력에서 받음(타인 명의 위조 가능) + 중복 무제한.
- read/update/delete: `BoardPostService::ap()`(boards/board_posts 컬럼 참조)를 조인 없는 reaction 쿼리에
  적용 → SQL 깨질 가능성. 통합테스트로 확정 필요.
- 작업: create는 세션 user 강제 + 보드 `reaction_visibility`/post 가시성 확인 + (post_id,user_id,value) 중복 정책.
  ap는 reaction 전용으로 재작성(board_post join 후 컬럼 참조).

### P0-5. 파일 수명주기 + 고아 파일 정리 (견고 방향 확정) ✅ 구현 완료 (DB 통합테스트 미작성)
구현됨: 엔티티/마이그레이션 PK 정정(file_uuid 단독, post_id nullable+pending 클레임), board 업로드 시
pending 등록(`register_pending_board_file`), 포스트 create/update 시 claim(명시 uuid + 본문 HTML 파싱,
`claim_post_files`/`extract_referenced_file_uuids`), refs 운영 + Active 승격/Temporary 강등
(`util/file_lifecycle.rs`), 포스트 삭제 시 `release_all_post_files`, reaper(`service/fs/maintenance.rs`)를
membership maintenance pass에 연결. 본문 파싱은 유닛테스트 5종 통과.
남은 후속: claim/unclaim/reaper의 **DB 통합테스트**(sumida_test 필요), grace(24h)를 settings로 외출,
"본문에서 빠진 인라인 이미지 자동 detach"는 보류(첨부 유지 → 깨짐 없음, 스토리지 잔류만).
설계 합의: `files`=범용 추상 테이블, `board_files`=정션, `files.refs`=도메인 무관 참조카운트.
tiptap는 글 생성 전 파일을 선업로드 → `board_files.post_id` nullable = "pending 클레임".
- (a) 엔티티 정정: `board_files` 복합 non-null PK 제거 → PK `file_uuid` 단독(1:1 유지), `post_id` nullable+인덱스.
- (b) board 전용 업로드: 정책 검증 후 `files`(status=temporary, expires_at=now+grace) + `board_files(post_id=NULL)` +
  `files.refs=1` 등록. 응답에 인라인 표시용 URL/엔드포인트 포함.
- (c) claim: 포스트 생성/수정 시 `attached_file_uuids` + **본문 HTML 파싱으로 참조 uuid 자동 수집**(견고 방향) →
  pending `board_files`를 UPDATE로 `post_id` 채움(현 delete+reinsert 폐기), `files.status=active`, `expires_at=NULL`.
- (d) refs 운영: 정션 insert/delete 시 `files.refs` ± (동일 트랜잭션).
- (e) 2단계 reaper(`membership::maintenance` 스케줄러 + advisory lock 재사용):
  A. `board_files WHERE post_id IS NULL AND created_at < now-grace` 삭제 → refs-- (제출-전-이탈 회수)
  B. `files WHERE refs=0 AND status='temporary' AND expires_at<now` 삭제(스토리지 객체 포함). 드리프트 방어로
     `AND NOT EXISTS(any junction)` 가드 추가.
- 참고: `files.refs`/`expires_at` 컬럼은 이미 존재하나 현재 미사용. 업로드는 status를 영구 `Temporary`로 둠.

### P1-6. 목록 스니펫(FTS)으로 비공개 글 본문 유출
- 위치: `board_post/mod.rs:75-90`(list) + `board_post/util.rs:84-110`(with_fts)
- list가 read ReBAC만 적용, post `content_visibility` 미반영 → 멤버전용/비공개 글 본문 일부가 스니펫으로 노출.
- 작업: 스니펫/snippet 컬럼을 가시성에 따라 마스킹(비공개 글이면 NULL).

### P1-7. 파일 인라인 서빙 + 업로더 권한
- 위치: `util/storage.rs:285-288`(local get_uri=서버 절대경로), `:379-381`(S3 get_uri=공개 URL),
  `:104`(download은 항상 attachment), `fs/file.rs:upload`(업로더 권한 부여 없음)
- 로컬 URI는 브라우저에서 사용 불가, S3 URI는 가시성/비밀글 우회. 인라인 표시 경로 없음.
- 작업: 인증/가시성 연동된 인라인 서빙 엔드포인트(또는 S3 서명 URL), 업로드 시 업로더 file 권한 부여.

### P1-8. MIME/확장자/길이 검증 강화
- 위치: `util/file_validation.rs:160-180`(블랙리스트만, 매직넘버 없음), content/title 길이 무제한
- 작업: 매직넘버 기반 콘텐츠 스니핑, content/title 최대 길이, (HTML 본문) 서버측 sanitize 검토(저장형 XSS).

### P2-9. 미완성 / 정합성
- 예약 발행: `should_be_published_now()`만 존재, 자동 발행 스케줄러 없음 → 예약글 미발행 (`board_posts.rs:204-214`).
- 반응 집계: `board_posts.reactions`(JSON)을 반응 생성 시 갱신 안 함 → 항상 stale.
- soft delete: `deleted_at`/status `Deleted` 있으나 `delete()`는 하드 삭제만 (`board_post/mod.rs:445-467`).
- `with_file_columns`가 `deleted_at`을 `UpdatedAt`에 매핑(오타) (`board_file/util.rs:15`).
- `boards` 엔티티 `defaultvalue` 오타로 기본값 어트리뷰트 무시 (`entity/src/bps/boards.rs:60,62`).
- 조회수: `board_post_views`에 (post_id,user_id) 유니크 없음 + read-then-write 경쟁 (`board_post/util.rs:207-240`).
- `board_post_reports` 테이블만 있고 엔티티/서비스/라우트 전무.
- 테스트: 글/파일/반응/업로드/가시성/비밀글 테스트 0건 (`api/tests/board.rs`는 boards 권한 9건만).

### 시나리오 테스트 작성 중 발견·수정 (2026-06) ✅
`api/tests/board_scenario.rs`(7 시나리오) 작성 과정에서 드러난 기존 버그를 함께 수정:
- **작성자 권한 자동부여가 깨져 모든 글 생성이 500**: `board_post/mod.rs`의 `ap::edit()`이 `.action()` 누락 +
  `effect`에 "update"/"delete"를 잘못 넣음 → `action must be set`. → action=["update","delete"], effect="allow",
  대상은 `ApUser::User`(role/org로 새지 않게)로 수정.
- **비작성자 글 조회가 500**: `with_conditional_post_columns`가 비작성자에게 `views`를 NULL로 반환하나
  DTO `views: i32`(non-null) → 디코딩 실패. → 비작성자 기본값 0으로 수정.
- **반응 컨트롤러가 라우터에 미연결**: `BoardPostReactionController`가 `lib.rs` nest 목록에 없어 HTTP로 도달 불가.
  → `/board-post-reactions`로 마운트.
- 신규 테스트: 작성자 CRUD, article 목록(공지 제외 회귀가드), 비공개 가시성, 비밀글(get/view 양쪽), 반응
  (userId 위조 무시·idempotent·소유자만 삭제), 파일 수명주기(업로드→pending→attach→reaper), 인라인 자동 claim.
  본문 파싱 유닛 5건 + 기존 board 권한 9건 모두 통과.

### 추천 후속 1차 진행 (2026-06) ✅
- **#6 `get_posts` 경로 board id 강제**: `controller/bps/board.rs`에 `Path(board_id)` 추출해 필터 주입.
- **#7 `board_post_view` 제거**: 미연결 + 조회기록 수동 CRUD는 footgun → 컨트롤러/서비스/dto 삭제.
  (후속: `boards.viewer_visibility` 연동 **읽기 전용** 뷰어 목록은 미구현 — 필요 시 신규 구현.)
- **#1 가시성 합성 min→max**: `check_post_visible`이 더 restrictive 우선(board가 조일 순 있어도 풀진 못함).
- **#2 목록/검색 가시성 필터**: `content_visible_filter`로 비가시 글을 list/FTS에서 행 자체 제외(제목·스니펫 유출 차단).
- **#3 인라인 서빙**: `GET /files/{uuid}/inline`(disposition=inline). board 첨부는 post 가시성으로, 그 외는 `file:read`로 인가.
  로컬 `get_uri` 절대경로 문제는 inline 엔드포인트로 우회(원본 `uri` 의존 제거).
- **#4 업로더 권한**: 업로드 시 업로더에게 `file:read` 부여(compose 중 미리보기 가능), 파일 삭제 시 정책 정리.
- **추가로 드러난 기존 버그**: `CreateBoardPost`/`UpdateBoardPost`의 `into_active_model`이 **`content_visibility`를
  set하지 않아** 모든 글이 DB기본값 `private`로 저장(요청 필드 무시)되던 문제 — DTO에 추가. (min→max 수정이 노출시킴.)
- 테스트: `board_scenario.rs` 8 시나리오(가시성 list 제외·인라인 서빙 공개/비공개 포함) 전부 통과.

### 추천 후속 2차 진행 (2026-06) ✅
완성도/하드닝 묶음 + 그 과정에서 드러난 기존 버그.
- **C-3** `with_file_columns`가 `deleted_at`을 `UpdatedAt`에 매핑하던 오타 → `DeletedAt`.
- **C-4** `boards` 엔티티 `defaultvalue` 오타(is_versioned/is_enabled) → `default_value`.
- **C-2** `board_post_views`에 `(post_id,user_id)` 유니크 인덱스 + 조회수 증가를 ON CONFLICT DO NOTHING으로
  경쟁 안전 처리(원자적 +1, lost update 제거).
- **C-5** reaper grace(24h) 하드코딩 → `storage.orphan_grace_hours` 설정으로 외출(예제 toml 문서화).
- **B-1** 예약 발행 잡: `bps::maintenance::publish_scheduled_posts`(scheduled+scheduled_at 경과 → published,
  published_at 스탬프)를 maintenance pass에 연결.
- **B-2** 반응 집계: 반응 create/update/delete 시 `board_posts.reactions`를 `{value: count}` 맵으로 갱신.
- **A-1** 파일/콘텐츠 하드닝: 업로드 매직넘버 검사(ELF/PE/Mach-O/shebang 차단), post `title`(255자)/`content`(1MB) 길이 제한.
- **추가로 드러난 기존 버그**: 파일 검증 에러(`UnsupportedMimeType`/`FileTooLarge`/`TooManyFiles`/`InvalidFileType`/
  `FileUploadNotAllowed`/`FileReuseNotAllowed`/`FieldNotProvided`)가 status 매핑 누락으로 **500**으로 떨어지던 것
  → 415/413/400으로 매핑(`error/src/error.rs`).
- 테스트: `board_scenario.rs` **11 시나리오**(+ 반응집계/실행파일거부/예약발행), board 권한 9건, 유닛 5건 모두 통과.

### 추천 후속 3차 진행 (2026-06) ✅
- **A-2 uri 게이트**: 응답에 노출하는 파일 경로를 내부 저장 URI 대신 읽기전용 게이트 경로
  `/files/{uuid}/inline`으로 가공(`fs::dto::inline_uri`). file list/get, board_file list/get/get_files_for_post,
  post 첨부(`load_attached_files`) 전부 적용. (남은 후속 A-2b: S3는 inline 핸들러에서 단기 pre-signed 302로 오프로드.)
- **B-3 신고**: `board_post_reports` 엔티티 + 서비스/컨트롤러/`/board-post-reports` 라우트. 인증 사용자가 가시 글을
  reason과 함께 신고(`(post_id,user_id)` 유니크 → 재신고는 reason 갱신), 모더레이터(`board_post_report` ns)가 목록/조회/삭제.
- **B-4 뷰어 목록**: `GET /board-posts/{id}/viewers` — `board.viewer_visibility`(public/members/author/private)로 게이트.
  board_post_views↔users 관계 추가. (content_visibility=읽기 / reaction_visibility=반응목록 / viewer_visibility=조회자목록 3분리 확인.)
- **C-1 soft delete**: board_posts `delete()`를 `deleted_at`+status=Deleted 마킹으로 전환(파일은 release).
  모든 read 경로에 `deleted_at IS NULL` 필터 — `default_query_with`(AP read 일괄) + raw find_by_id 6곳(get/view/update/
  list_viewers/reaction/report/inline). board는 범위 외(컬럼 미사용 유지).
  알려진 한계: 부모 soft-delete 시 대댓글(자식 post)은 개별 유지 — 필요 시 cascade soft-delete 후속.
- 테스트: `board_scenario.rs` **15 시나리오**(+신고/뷰어목록/soft-delete/첨부 uri 게이트) 전부 통과.

### 남은 후속
- **A-2b** S3 inline pre-signed URL 302(성능 오프로드) — 인프라 결정.
- 대댓글 cascade soft-delete, board soft delete(현재 board는 하드 유지).

## P2 — 정합성 / 정리

### examples/basic의 stale 바이너리 복구
- `examples/basic/src/bin/create_dummy_board_posts.rs` 컴파일 에러
- 원인: `OrganizationsActiveModel.description/is_enabled`, `RolesActiveModel.level/is_enabled` 등
  entity 단순화 이후 따라가지 못함
- 영향: `cargo check --all-targets`가 깨짐. lib 빌드와 `app` 바이너리는 영향 없음
- 진단: 단순 dead-code 갱신. 30분 내 작업

### 운영 정책 시드 가이드 문서화
- G7에서 도입한 `namespace="org:user"` 권한을 운영 DB에 시드해야 함:
  `(namespace="org:user", object="<org_id>", action=["update"], effect="allow", user="role:<org_admin_role_id>")`
- 시드 누락 시 `is_admin=false` 사용자의 `PATCH /users/{id}`는 항상 403
- `README.md` 또는 `docs/permissions.md`에 절차 + SQL 예시 명시

## P3 — 갈라파고스 정리 / 표준화 (사용자 합의 후)

### Figment 마이그레이션
- 현재 `infra/src/settings/mod.rs`는 `config` crate + lazy_static
- 마이그레이션 시 환경변수 override / SSM provider 통합 / 부팅 시 오류 누적 가능
- 전제: e2e 회귀 테스트가 동일 settings.toml에 대해 같은 deserialize 결과를 확인

### RFC 7807 Problem Details 응답 전환
- 현재 응답: `{ code, statusCode, reason? }` (sumida 자체 스키마)
- 표준 전환 시 클라이언트 호환성 영향 — major 버전 변경 등 협의 필요
- 전환 위치: `error/src/error.rs::SumidaApiError::to_response` 한 곳

### ReBAC ApOr / object_in_subquery 사용처 확대
- 현재 `ApOr`와 `object_in_subquery`는 `UserService`만 사용
- 게시판 등 "owner OR moderator" 같은 OR 정책이 필요한 곳에 점진 적용
- 적용 전 deny 시멘틱(OR 분기 시 자식 간 deny 전파 없음) 운영 정책 명문화

### G5b 후속 — token 관련 코멘트/주석 정리
- `service/auth/token.rs` 상단 doc 보강 (login/refresh/logout 흐름과 token_type 검증 요점)
- `controller/auth/token.rs`에 OpenAPI 스타일 doc-comment 추가 (다운스트림 클라이언트가 참고)

## P2 — 멤버십 후속 (대형 / 외부·제품 의사결정 필요)

멤버십·결제 도메인의 1차 구현은 완료(`docs/membership.md`). 남은 항목은 외부 의존성
또는 제품 결정이 필요해 보류함.

### ~~빌링키 발급(issuance) 서버 연동~~ — 결정: 수동/클라이언트 주입 (완료)
- 결정: 빌링키는 클라이언트 SDK가 발급(카드정보 서버 미경유) → 문자열을 서버에 주입.
  `POST /subscriptions`(billingKey) / `POST /subscriptions/{id}/auto-renew`로 주입 가능.
- 주입 시 `PaymentProvider::verify_billing_key`로 PG 검증(PortOne `GET /billing-keys/{key}`,
  Mock/None 대응) → 잘못된 키는 즉시 400. `BillingKey.Issued` 웹훅은 불필요로 폐기.

### per-currency 크레딧 월렛 / 환율
- 현재: 크레딧 월렛은 owner당 1개·단일 통화(통화 필드는 정보성). 통화는 plan/purchase/coupon에만 의미
- 진짜 멀티커런시는 통화별 잔액(스키마 변경) + 환율 정책 필요. (원안에서도 "과설계"로 분류)

### 크레딧 reserve/capture ↔ PG authorize-capture 연결
- 현재: 내부 원장 기반 hold만. 외부 결제 승인-매입과는 분리
- 비동기 결제(가상계좌 등)와 묶을 때 연동 검토

### 소소한 항목
- ~~조직 owner 알림 채널~~ — 완료: org 관리자(org:subscription/org:credit manage 정책 보유자) 이메일로 발송
- ~~스케줄러 다중 인스턴스 advisory lock~~ — 완료: `run_once`가 Postgres advisory lock으로 단일 러너 보장
- 쿠폰은 PG checkout 경로에만 적용(무료 subscribe 경로 제외) — 의도된 분리, 필요 시 확장
