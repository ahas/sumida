---
description: "Conventional Commits 규칙에 따라 자동으로 git commit 수행"
argument-hint: "[optional-scope]"
---

현재 변경된 파일들의 내용을 확인하고 Conventional Commits 규칙을 따라 적절한 커밋 메시지로 git commit을 수행해주세요.

Conventional Commits 형식:
```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

**Types:**
- `feat`: 새로운 기능 추가
- `fix`: 버그 수정
- `docs`: 문서 변경
- `style`: 코드 포맷팅, 세미콜론 누락 등 (기능 변경 없음)
- `refactor`: 리팩토링 (기능 변경이나 버그 수정 없음)
- `test`: 테스트 추가 또는 수정
- `chore`: 빌드 프로세스, 보조 도구 변경 등

**Scope (선택사항):** $1

**작업 순서:**
1. `git status`로 변경된 파일 확인
2. `git diff`로 변경 내용 분석
3. 변경 사항에 적합한 type과 description 결정
4. Conventional Commits 형식으로 커밋 메시지 작성
5. `git add .`로 파일 스테이징
6. `git commit`으로 커밋 실행

**참고사항:**
- 한 번에 여러 type의 변경이 있다면 가장 중요한 변경사항 기준으로 결정
- 설명은 현재형, 명령문으로 작성 (예: "add feature" not "added feature")
- 50자 이내로 간결하게 작성
- Breaking change가 있다면 footer에 `BREAKING CHANGE:` 추가