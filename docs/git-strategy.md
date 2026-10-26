# Git 전략

MVP 2부터 적용한다. 원격 저장소는 `git@github.com:rlaskaehd/bareframe.git`(private)이며, MVP 1은 `main`의 `mvp-1` 태그로 기록했다.

이 저장소에는 다음과 같은 성격이 있어서, 짧은 작업 브랜치와 squash merge를 쓰는 trunk 기반 흐름을 택한다.

- 개발자 한 명과 Claude 세션 여러 개(Worker, Reviewer)가 함께 작업한다.
- `vendor/`에 upstream 수정 사본이 있다.
- GUI 검증은 수동으로 한다.

## 원칙

- `main`은 항상 `./script/check`를 통과하고, Shell Integration이 없어도 일반 터미널로 동작한다.
- 모든 변경은 작업 브랜치에서 PR을 거쳐 `main`에 squash merge한다. `main`에 직접 push하지 않는다.
- 브랜치는 하나의 목적만 다루고 며칠 안에 merge한다. 오래 유지하는 통합 브랜치(`develop`, `mvp2`)는 만들지 않는다.
- MVP 단위의 완료는 태그로 표시한다.

## 브랜치

형식은 `<type>/<topic>`이다. MVP 작업이면 topic 앞에 `mvpN-`을 붙인다.

| type | 용도 | 예 |
| --- | --- | --- |
| `feat` | 기능 추가 | `feat/mvp2-osc133-events` |
| `fix` | 결함 수정 | `fix/ime-unmark-commit` |
| `vendor` | `vendor/gpui-libghostty` 변경과 upstream 갱신 | `vendor/command-boundary-api` |
| `docs` | 문서만 변경 | `docs/mvp2-plan` |
| `chore` | 스크립트, 도구, 의존성 | `chore/ci-check-core` |

merge 전에 `main` 위로 rebase하고 `./script/check`를 다시 실행한다. merge된 브랜치는 GitHub이 자동으로 삭제한다.

## 커밋

Conventional Commits 형식을 쓰고, 요약은 한국어로 써도 된다.

```text
feat(core): Block 상태 모델 추가
fix(adapter): 조합 중 unmark_text가 확정하도록 수정
vendor: OSC 133 경계 이벤트를 관찰 큐로 전달
```

scope는 `core`, `app`, `adapter`, `vendor`, `script`, `test`, `docs`에서 고른다.
작업 브랜치 안의 커밋은 자유롭게 나눠도 된다. squash merge 때 PR 제목이 `main`의 커밋 메시지가 되므로, PR 제목을 같은 형식으로 쓴다.

## PR과 세션 역할

| 단계 | 담당 | 내용 |
| --- | --- | --- |
| 작업 | Worker | 브랜치 생성 → 구현 → `./script/check` → `gh pr create`(템플릿 사용) |
| 리뷰 | Reviewer | `gh pr diff`로 리뷰하고 필요하면 GUI 재검증. 결과는 `gh pr review --comment`로 남긴다 |
| 수정 | Worker | 리뷰 지적을 같은 브랜치에 커밋 |
| merge | 오케스트레이터 세션 또는 사용자 | 리뷰 결론이 승인이고, head가 승인한 SHA와 같고, check가 통과하면 `gh pr merge --squash` |

모든 세션이 같은 GitHub 계정을 쓰므로, 자기 PR에 GitHub의 approve를 할 수 없다.
그래서 승인은 Reviewer 코멘트의 첫 줄로 판단한다. 첫 줄은 `결론: 승인 (<head SHA>)` 또는 `결론: 수정 필요 (<head SHA>)` 형식으로 쓴다.
승인 뒤에 커밋이 추가되면 그 승인은 무효이고, 다시 리뷰를 받는다.

- 작업 브랜치에 커밋하고 rebase·force-push하는 것은 그 브랜치를 맡은 Worker만 한다. Reviewer는 지적을 코멘트로 돌려보낸다.
- Reviewer 세션에 `gh pr review`·`gh pr comment` 권한이 없으면, 리뷰 전문을 파일로 남기고 결론을 오케스트레이터에게 메시지로 보낸다. 이때 PR 게시와 merge 여부는 사용자가 정한다.

## vendor 변경

- `vendor/gpui-libghostty`는 앱이 실제로 쓰는 수정 사본이다. `reference/`는 추적하지 않는 읽기용 스냅샷이다(index와 lock 파일만 추적).
- vendor를 고치면 같은 PR에서 `vendor/gpui-libghostty/BAREFRAME.md`에 변경 항목을 추가한다.
- 앱 코드와 vendor 변경은 **PR을 나눈다**. squash merge는 PR 안의 커밋을 하나로 합치므로, 커밋만 나누면 `main`에는 구분이 남지 않는다. PR을 나눠야 다음 upstream 갱신 때 로컬 변경을 하나씩 다시 적용할 수 있다.
- upstream 갱신은 `vendor/update-<short-rev>` 단독 PR로 한다. 사본을 교체하고, `BAREFRAME.md`의 로컬 변경을 다시 적용하고, MVP 1 수용 행렬의 입력·IME·종료 항목을 다시 검증한다.

## 의존성

- `Cargo.lock`은 추적하며, 의존성을 바꾸는 PR에서만 변경한다. 그 밖의 검사는 `--locked`로 실행한다.
- Python 의존성은 모두 `requirements.txt`에 적는다. 실행 환경은 `mamba` `claude`다.
- `target/`, `.cache/`, `.tools/`는 추적하지 않는다.
- `script/`는 직접 실행한다. `script/env`는 bash에서만 source할 수 있고, zsh에서 source하면 오류로 멈춘다. 프로젝트 밖에 빌드 결과물이 생기는 것을 막기 위해서다.

## 검증 기록

- `target/validation/`의 근거 파일은 추적하지 않는다.
- 결과 요약은 `docs/mvp-N-validation.md`에 쓰고, 같은 PR에 포함한다.
- GUI 검증을 하지 못한 PR은 본문의 "미검증" 항목에 남긴다. merge 뒤 검증하면 결과를 별도 `docs/` PR로 반영한다.

## 태그

- 로드맵의 완료 기준을 만족하고 검증 기록에 FAIL이 없을 때, `main`에 `mvp-N` annotated tag를 단다.
- 외부 배포를 시작하면 `Cargo.toml`의 version과 맞춘 `vX.Y.Z` 태그를 추가로 도입한다. 지금은 쓰지 않는다.

## CI

아직 두지 않는다. 전체 빌드에 macOS GPU, Zig, 큰 네이티브 의존성이 필요하고, 핵심 검증이 GUI 수동 검증이기 때문이다.
PR을 만들기 전에 `./script/check`를 로컬에서 통과시키는 것으로 대신한다.
협업자가 늘거나 check 누락으로 `main`이 깨지면, macOS runner에서 `./script/check-core`부터 도입한다.
