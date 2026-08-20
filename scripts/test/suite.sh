#!/usr/bin/env bash
#
# 통합 테스트를 한 번에 하나씩 돌린다.
#
# 이 스위트의 하네스는 각 테스트 바이너리가 시작할 때 데이터베이스를 통째로 지우고 다시 세운다
# (`Migrator::fresh`). 그런데 `cargo test` 는 테스트 바이너리 여러 개를 동시에 띄우므로, 한 쪽이
# 한창 돌고 있는 사이 다른 쪽이 그 테이블을 지운다. 증상은 매번 다른 테스트가 401·403·500 으로
# 떨어지는 것이고, 같은 바이너리를 혼자 돌리면 멀쩡히 통과한다. 어느 것이 진짜 실패인지 분간이
# 안 되는 상태가 된다.
#
# 그래서 대상을 하나씩 넘긴다. 유닛 테스트는 데이터베이스를 쓰지 않으므로 그대로 한 번에 돈다.
#
# 사용: bash scripts/test/suite.sh [cargo test 에 넘길 추가 인자...]
#
#   SUMIDA_TEST_FEATURES=...   활성화할 feature (기본: e2e/system-settings)
#
# 전제: Postgres 와 Redis 가 떠 있고, `.env.e2e` 가 가리키는 데이터베이스 이름이 `_test` 로 끝난다
#       — 하네스가 그것을 확인하고, 아니면 멈춘다. 이 스위트는 매 실행마다 모든 표를 지운다.

set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1

FEATURES="${SUMIDA_TEST_FEATURES:-e2e/system-settings}"

failed=()

run() {
  local label="$1"
  shift

  printf '\n\033[1m── %s\033[0m\n' "$label"

  if ! cargo test --features "$FEATURES" "$@" "${EXTRA[@]}"; then
    failed+=("$label")
  fi
}

EXTRA=("$@")

# 데이터베이스를 건드리지 않는 것들. 함께 돌아도 서로를 밀어내지 않는다.
run "unit" --workspace --exclude basic --lib

# 통합 테스트 대상. 하나씩.
for target in $(find api/tests -maxdepth 1 -name '*.rs' -printf '%f\n' | sed 's/\.rs$//' | sort); do
  run "api::$target" -p api --test "$target"
done

printf '\n'

if [ ${#failed[@]} -eq 0 ]; then
  printf '\033[32m전부 통과\033[0m\n'
  exit 0
fi

printf '\033[31m실패: %s\033[0m\n' "${failed[*]}"
exit 1
