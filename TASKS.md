# Tasks / Roadmap

아직 열려 있는 일만 적는다. 마친 항목은 지운다 — 무엇이 언제 왜 바뀌었는지는 git 과 changelog 의
것이고, 남겨 두면 이 파일이 코드와 어긋난 채로 사람을 잘못 이끈다. 끝난 일이 남긴 결론 중 계속
지켜야 하는 것은 `docs/` 의 해당 문서에 **지금의 제약**으로 적는다.

작업 단위가 커지면 별도 `TASKS_*.md` 로 나눈다.

## 돌지 않는 테스트

### DynamoDB JWT 스토어 라운드트립 — 기본 스위트 밖
- 스토어는 둘 중 하나만 컴파일된다(`cfg_if`, 기본은 redis). 그래서 이 경로는 redis 를 끄고
  dynamodb 를 켠 빌드에서만 존재하고, `scripts/test/suite.sh` 에는 들어오지 않는다.
- 표는 `pk`/`sk` 둘 다 문자열 키에 TTL 속성이 `ttl` 이다. 그 표와 AWS 자격을 갖추고 아래를
  돌리면 저장·조회·삭제와 블랙리스트 왕복이 실제 DynamoDB 에서 확인된다.

  ```
  SUMIDA_TEST_DYNAMO_TABLE=<표 이름> \
    cargo test -p infra --no-default-features \
      --features session-store-aws-dynamodb,jwt dynamo_jwt_store_roundtrip
  ```

### 고아 파일 정리의 S3 백엔드 — 기본 스위트 밖
- 로컬 파일시스템 백엔드는 `api/tests/file_lifecycle.rs` 가 기본 스위트에서 확인한다. S3 는 같은
  트레이트의 다른 구현이고 버킷이 있어야 돌므로 `api/tests/file_lifecycle_s3.rs` 로 따로 선다.
- 버킷과 AWS 자격을 갖추고 아래를 돌린다. 두 변수 모두 필요하다 — 하나는 스토어를 그 버킷으로
  가리키고, 하나는 케이스가 실제로 돌아도 되는지를 정한다. 첫 케이스가 스토어의 백엔드를 먼저
  확인하므로, 설정이 빠지면 나머지가 로컬에서 통과하며 S3 에 대해 아무 말도 하지 않는 일은
  생기지 않는다.

  ```
  SUMIDA_TEST_S3_BUCKET=<버킷> SUMIDA_STORAGE__S3__BUCKET=<버킷> \
    cargo test --features e2e/system-settings,api/storage-aws-s3 \
      -p api --test file_lifecycle_s3
  ```

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
