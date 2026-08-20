# Tasks / Roadmap

아직 열려 있는 일만 적는다. 마친 항목은 지운다 — 무엇이 언제 왜 바뀌었는지는 git 과 changelog 의
것이고, 남겨 두면 이 파일이 코드와 어긋난 채로 사람을 잘못 이끈다. 끝난 일이 남긴 결론 중 계속
지켜야 하는 것은 `docs/` 의 해당 문서에 **지금의 제약**으로 적는다.

작업 단위가 커지면 별도 `TASKS_*.md` 로 나눈다.

## 돌지 않는 테스트

### DynamoDB JWT 스토어 라운드트립
- `infra/src/jwt.rs` 의 `dynamo_jwt_store_roundtrip` 은 DynamoDB 스토어 빌드(redis off)에서만
  컴파일되고, `SUMIDA_TEST_DYNAMO_TABLE` 이 실제 표를 가리킬 때만 돈다. 그 표가 없으면 조용히
  건너뛰므로, 이 경로는 지금 아무 데서도 확인되지 않는다.
- 실행: `cargo test -p infra --no-default-features --features session-store-aws-dynamodb dynamo_jwt_store_roundtrip`
  (AWS 자격·리전 + pk/sk(string) + `ttl` 속성 표 필요)

### 파일 수명주기의 DB 통합 테스트
- 고아 파일 정리와 청구(claim) 흐름은 구현돼 있으나 데이터베이스를 실제로 거치는 통합 테스트가
  없다. 유예 시간이 지난 파일이 실제로 지워지는 경로가 확인되지 않는다.

## 결제 — 2단계 결제 (제품 결정 + 샌드박스 필요)

`PaymentProvider` 에 `authorize`/`capture`/`void` 가 서 있고 Mock 은 완전히 구현돼 있다. 남은 것은
둘이다.

- **서비스 흐름** — 언제 authorize 에서 capture 로 넘어가는지, 그 사이에 무엇이 예약된 상태인지를
  정해야 엔드포인트(`/payments/authorize|capture|void`)와 `membership_purchases` 의
  `provider_auth_code` 컬럼이 결정된다. 즉시 결제가 기본이고 2단계는 선택으로 얹는다.
- **PortOne V2 manual-capture** — 위가 정해진 뒤 실제 엔드포인트로 채운다. 샌드박스 계정이 있어야
  검증된다.

결제창에 카드를 입력해야 하는 경로(일회성·크레딧 checkout 의 실 paymentId verify, webhook 실발송)는
서버만으로 자동화되지 않는다.

## 목록 필터 계약 — 남은 리소스와 후속 (정본: `docs/list-filters.md`)

sumida 의 최상위 목록 리소스는 전부 선언을 들고 `GET <리소스>/filters` 를 내주며, 정렬과 조건
트리를 그 선언 하나로 푼다. 아래는 아직 닿지 않은 부분이다.

### 지갑 하위 목록 세 개가 아직 선언을 갖지 않는다
- `/coupons/{id}/grants`, `/coupons/mine`, 크레딧 원장(`credit_transactions`)과 예약
  (`credit_reservations`) 목록은 손으로 적은 정렬 whitelist 를 그대로 쓰고 서술을 내주지 않는다.
- 원장·예약의 `walletId` 는 **질의 앞에서 답해지는 스코프 선택자**다(월렛 소유자 확인이 거절로
  끝난다). 선언에 넣으면 그 거절이 빈 목록이 되므로, 계약에 넣을 때도 이 파라미터는 필드가
  아니어야 한다.

### 범위 파라미터가 없어 날짜·숫자 키가 전부 정렬 전용이다
- 선언에서 `between`/`gte`/`lte` 를 여는 순간 호출자는 `a..b` 표기를 보내는데, sumida 의 필터
  구조체 중 `Bounds` 를 든 것이 없다. 접근 로그의 기간 조회처럼 실제 수요가 생기면 **먼저
  파라미터를 `Bounds` 로 바꾸고** 그다음 연산자를 연다.

### 표시 순서를 나르는 `order` 컬럼은 이름을 바꾸지 않으면 선언할 수 없다
- boards / board_topics / board_groups 가 `order` 컬럼으로 화면 순서를 정하는데 `order` 는 목록
  봉투가 이미 가진 예약 키다. 정렬을 열려면 다른 키 이름을 붙여야 한다.

### 정렬 키 표기가 코어와 bps 에서 snake_case, 그 밖에서 camelCase 로 갈려 있다
- 한 컬럼에 키는 하나이므로, 다른 표기로 보낸 `sort` 는 오류 없이 기본 정렬로 떨어진다. 표기를
  한쪽으로 모으는 것은 클라이언트와 함께 움직여야 하는 별도 작업이다.

## 규칙이 정본이 아닌 자리

### 쿠폰 관리 권한만 튜플로 표현되지 않는다
- `CouponService` 는 세션의 `is_admin` 플래그를 직접 읽고 `OnlyAdminAllowed` 로 거절한다. 그래서 이
  권한만은 부여할 수도, 위임할 수도, 조직으로 좁힐 수도, `/me/access-policies` 로 볼 수도 없다.
- 네임스페이스 하나(`membership_plan` 과 같은 모양)로 옮기거나, 규칙 집합이 "누가 무엇을 할 수
  있는지의 단일한 진술"이라는 주장을 좁히거나 — 둘 중 하나를 정해야 한다.

## 문서와 주석이 코드를 가리키는 방식

### `설계 §N` 이 가리키는 문서가 없다
- `api/src` 의 주석 쉰아홉 곳이 "설계 §5.1", "design §9.7", "E1" 같은 절 번호를 인용한다. 그런
  문서는 이 저장소에 없다. 가리키는 내용을 실제 문서로 올려 이름으로만 부르거나, 참조를 걷어낸다.

### 규칙 파일과 주석이 변경 이력과 소스 좌표를 나른다
- `.sz` 규칙 파일은 권한 모델의 정본인데 "종전에는 …" 서술과 `membership/credit/mod.rs:88` 같은
  파일·줄 좌표를 담고 있다. 좌표는 소스가 움직이는 순간 어긋나고, 이력은 지금의 제약이 아니다.
  서른 곳 남짓의 주석이 같은 형태다. 한 번에 쓸어야 표기가 반쪽만 바뀌지 않는다.

## 죽은 표면

`access_policies` 표는 init 마이그레이션이 여전히 만들지만 읽는 코드가 없다. 두 묘비 마이그레이션
(`rebac_backfill`, `rebac_mirror`)이 그 표가 비어 있음을 확인하고 트리거를 걷어낸 상태다. 이름이
권한을 뜻하는 표가 살아 있으면 그 표에 심는 실수를 부른다 — 비어 있음이 확인된 뒤 떨어뜨린다.

`AdminService` 와 빈 `admin/dto.rs`, `util::db::i32_or_struct`, `capability::has`, `service_ap_new!`,
`layer::board_ap::helpers`, `board_file::dto::{AttachFileToPost, DetachFileFromPost}` 는 모두 호출자가
없다. 마지막 둘은 `board_files` 의 키(`file_uuid` 하나, `post_id` 는 nullable)와 모양이 어긋나
있기도 하다.
