# 권한(ReBAC) 모델 & 시드 가이드 — 게시판(bps) / 파일(fs)

`sumida`의 모든 보호 라우트는 단일 테이블 `access_policies`로 운영되는 ReBAC로 통제된다.
**운영 DB에 시드 정책을 넣지 않으면 `service_ap` 레이어에서 전부 403**이 된다.
(전체 ReBAC 동작은 [`CLAUDE.md`](../CLAUDE.md) §2 참고. 이 문서는 게시판/파일 도메인의 시드 실무를 다룬다.)

## 1. 통제 계층 3종

요청이 핸들러에 닿기까지 거치는 관문:

1. **`user_guard`** — 세션 쿠키 또는 `Authorization: Bearer <jwt>` / `APIKEY <key>` 검증. 로그인 필요.
2. **`service_ap!(ns, action)`** — `(namespace=ns, object="service", action=action)` 정책을 통과해야 핸들러 진입.
   즉 **각 네임스페이스마다 `object="service"` 정책이 사용자(또는 그 role/org)에게 있어야** 라우트에 들어간다.
3. **행(row) 단위 검사** — 핸들러 내부:
   - 글 목록/조회: `board_group:read` + `board:read`를 해당 board/group 객체에 대해 통과한 글만 보임(+ post `content_visibility`).
   - 글 생성: `board_group:create object=<group_id>`(그룹 있으면) **AND** `board:create object=<board_id>`.
   - 글 수정/삭제: 운영자(`board_group`+`board`) **OR** 작성자 소유권(`board_post:update|delete object=<post_id>`, 생성 시 자동 부여).
   - 반응/신고/뷰어/인라인: 서비스에서 가시성·소유권으로 판정(아래 표).

`is_admin=true` 사용자는 위 전부를 우회한다.

## 2. 네임스페이스 / 액션 / 객체

| 네임스페이스 | 액션 | service-level 필요? | 추가 행 단위 검사 |
|---|---|---|---|
| `board` | read/create/update/delete | ✅ | 객체=board id. 글 생성·조회 체인에서도 사용 |
| `board_group` | read/create/update/delete | ✅ | 객체=group id. 글 생성·조회 체인에서도 사용 |
| `board_topic` | read/create/update/delete | ✅ | 객체=topic id |
| `board_post` | read/create/update/delete | ✅ | 생성=board/group 체인, 수정/삭제=운영자 OR 작성자(자동부여), 조회=board/group 체인 + `content_visibility` |
| `board_post_reaction` | read/create/update/delete | ✅ | board `reaction_visibility` + 소유자만 수정/삭제. 생성 시 user_id=세션 |
| `board_post_report` | read/create/delete | ✅ | create=가시 글 + 인증(누구나 신고 가능하게 시드), read/delete=모더레이터 |
| `board_file` | read/create/delete | ✅ | board 업로드 정책(`can_upload_file` 등) + 행 단위 권한 |
| `file` | read/create/delete | ✅ | 단, `GET /files/{uuid}/inline`은 service_ap 없이 **post 가시성/`file:read`로 행 단위 인가** |

`who`(정책의 `user` 컬럼) 형식: `user:<id>` | `role:<id>` | `org:<id>` | `api:<KEY>` | `user:admin` | `*`.
**반드시 `ApUser::*.to_string()` 형식으로 저장**(예: `role:2`).

## 3. 라우트 → 필요 권한 요약

| 라우트 | service_ap | 비고 |
|---|---|---|
| `GET /boards`, `/boards/{id}`, `/boards/{id}/posts` | `board:read` | 글 목록은 board 컨트롤러가 제공 |
| `POST/PATCH/DELETE /boards` | `board:create|update|delete` | |
| `GET /board-posts`, `/board-posts/{id}`, `/board-posts/{id}/view`, `/board-posts/{id}/viewers` | `board_post:read` | viewers는 board `viewer_visibility`로 추가 게이트 |
| `POST /board-posts` | `board_post:create` (+ board/group 체인) | |
| `PATCH/DELETE /board-posts/{id}` | `board_post:update|delete` (운영자 OR 작성자) | 삭제는 soft-delete(대댓글 cascade) |
| `GET/POST/PATCH/DELETE /board-post-reactions` | `board_post_reaction:read|create|update|delete` | |
| `GET/POST/DELETE /board-post-reports` | `board_post_report:read|create|delete` | create는 일반 사용자, read/delete는 모더레이터 |
| `POST /board-files`, `GET /board-files`, `DELETE /board-files` | `board_file:read|create|delete` | |
| `GET /files`, `/files/{uuid}`, `POST /files`, `DELETE /files/{uuid}` | `file:read|create|delete` | |
| `GET /files/{uuid}/inline` | (service_ap 없음) | 인증 + post 가시성/`file:read`로 행 단위 인가 |

## 4. 시드 예시 (SQL)

`access_policies(namespace, object, action, effect, "user")` 직접 INSERT 예시.
`action`은 `text[]`, `effect`는 `allow|deny`.

### 4-1. 일반 회원 역할 (role:2) — 읽기 + 글/반응/신고 작성

```sql
-- 게시판/그룹/토픽 읽기 (글 조회 체인이 board_group:read + board:read 를 요구)
INSERT INTO access_policies (namespace, object, action, effect, "user") VALUES
  ('board',       'service', '{read}',          'allow', 'role:2'),
  ('board',       '*',       '{read}',          'allow', 'role:2'),
  ('board_group', 'service', '{read}',          'allow', 'role:2'),
  ('board_group', '*',       '{read}',          'allow', 'role:2'),
  ('board_topic', 'service', '{read}',          'allow', 'role:2'),
  ('board_topic', '*',       '{read}',          'allow', 'role:2'),
-- 글 읽기/쓰기
  ('board_post',  'service', '{read,create}',   'allow', 'role:2'),
  ('board',       '*',       '{create}',        'allow', 'role:2'),  -- 글 생성 체인: board:create
  ('board_group', '*',       '{create}',        'allow', 'role:2'),  -- 글 생성 체인: board_group:create
-- 반응 / 신고 / 파일
  ('board_post_reaction', 'service', '{read,create,update,delete}', 'allow', 'role:2'),
  ('board_post_report',   'service', '{create}',                    'allow', 'role:2'),
  ('board_file',          'service', '{read,create}',               'allow', 'role:2'),
  ('file',                'service', '{read,create}',               'allow', 'role:2');
-- 글 수정/삭제는 작성자 소유권 정책이 생성 시 자동 부여되므로 별도 시드 불필요.
-- (단 service_ap 통과를 위해 board_post:update,delete service 정책이 필요하면 아래 추가)
INSERT INTO access_policies (namespace, object, action, effect, "user") VALUES
  ('board_post', 'service', '{update,delete}', 'allow', 'role:2');
```

> 글 수정/삭제: 라우트는 `board_post:update|delete` **service-level**을 요구하고(2계층), 핸들러는
> 운영자 권한 **또는** 작성자 소유권(생성 시 자동 부여된 `board_post:update|delete object=<post_id>`)을 본다.
> 따라서 일반 회원도 자기 글을 고치려면 `board_post:update|delete` service 정책이 있어야 라우트에 진입한다.

### 4-2. 모더레이터 역할 (role:3) — 신고 처리 + 글 운영

```sql
INSERT INTO access_policies (namespace, object, action, effect, "user") VALUES
  ('board_post_report', 'service', '{read,delete}',          'allow', 'role:3'),
  ('board_post',        'service', '{read,update,delete}',   'allow', 'role:3'),
  ('board',             '*',       '{read,update,delete}',   'allow', 'role:3'),
  ('board_group',       '*',       '{read,update,delete}',   'allow', 'role:3');
```

### 4-3. 관리자

`users.is_admin = true`면 ReBAC를 전부 우회한다. 별도 정책 불필요.

### 4-4. 게시판 운영 위임 (특정 board만)

특정 board(예: id=5)와 그 그룹(예: id=1)만 운영하게 하려면 `*` 대신 객체 id로 좁힌다:

```sql
INSERT INTO access_policies (namespace, object, action, effect, "user") VALUES
  ('board',       '5', '{read,create,update,delete}', 'allow', 'user:42'),
  ('board_group', '1', '{read,create,update,delete}', 'allow', 'user:42'),
  ('board_post',  'service', '{read,create,update,delete}', 'allow', 'user:42');
```

## 5. 주의

- **시드 누락 = 403.** 새 네임스페이스(예: `board_post_report`)를 도입하면 운영 DB·다운스트림 시드 스크립트를 같이 갱신한다.
- `deny`는 `allow`보다 우선한다(특정 객체 차단에 사용).
- `*` 와일드카드는 객체/액션 양쪽에 쓸 수 있다(`object='*'`, `action='{*}'`).
- 정책 관리 API: `/access-policies` 라우트 또는 `ap::edit()` 헬퍼(`api/src/util/ap.rs`).
- 파일은 응답·본문에 **항상 `/files/{uuid}/inline`(읽기전용 게이트 경로)** 만 노출한다(presigned/스토리지 URL 금지 — [`TASKS.md`](../TASKS.md) "파일 접근 원칙").
