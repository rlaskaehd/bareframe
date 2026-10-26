# MVP 2 구현 계획 — 명령 Block

기준: `main`의 `mvp-1` 태그(2026-10-26). 범위와 완료 기준은 [로드맵](roadmap.md#mvp-2--명령-block)을, 이벤트 경계는 [MVP 2 확장 경계](mvp-2-boundary.md)를 따른다.
작업 방식은 [Git 전략](git-strategy.md)을 따른다.

## 현재 상태 조사 결과

vendored Ghostty(`vendor/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty`)를 조사한 결과다. 경로는 그 아래 `src/` 기준이다.
아래 내용은 소스를 읽고 판단한 것이며, B0에서 실제 실행으로 확인한다.

| 항목 | 상태 | 근거 |
| --- | --- | --- |
| OSC 133 파싱(A/B/C/D, exit code) | 있음 | `terminal/osc/parsers/semantic_prompt.zig`, `termio/stream_handler.zig:993-1027` |
| 행·셀 semantic 표시(prompt/input/output) | 있음, 내부 전용 | `terminal/page.zig:1985`, `:2094` |
| 직전 명령 출력 선택 | 있음, 내부 전용 | `Screen.selectOutput`, `PageList.promptIterator` |
| 앱으로 전달되는 명령 이벤트 | `COMMAND_FINISHED`(exit code, duration)만 | `Surface.zig:1141-1170`, `include/ghostty.h:976` |
| 명령 문자열 | 없음. zsh 스크립트의 `133;C`가 문자열을 보내지 않음 | `shell-integration/zsh/ghostty-integration` |
| Alternate Screen 알림·getter | 없음 | `Terminal.zig:4783`, C API 목록 |
| 텍스트 읽기 C API | 화면 텍스트만. 스크롤백 임의 구간은 지정할 수 없음 | `apprt/embedded.zig:1380-1404`, `:1663` |
| bareframe 실행에서 셸 통합 | **꺼져 있음(추정)** | 아래 두 원인 |

셸 통합이 꺼져 있다고 보는 원인은 두 가지다.

1. **resources dir이 없다.** `-Demit-terminfo=false` 빌드라서 resources dir의 표식 파일이 없다. 그러면 `termio/Exec.zig:799`에서 통합이 꺼진다.
2. **셸 감지가 실패한다.** `config.rs`의 `command()`가 셸 경로를 `'/bin/zsh'`처럼 작은따옴표로 감싼다. 그런데 Ghostty의 셸 감지는 작은따옴표를 처리하지 않아서 basename이 `zsh'`가 되고, zsh로 인식되지 않는다.

`--clean-shell`(`zsh -f`)은 `.zshenv`를 읽지 않으므로 이 방식으로는 통합을 켤 수 없다.

## 설계 결정

- **통합 주입:** shim이 surface 환경 변수에 `ZDOTDIR`을 지정한다. 값은 앱이 배포한 vendored zsh 통합 디렉터리다. 사용자에게 원래 `ZDOTDIR`이 있으면 `GHOSTTY_ZSH_ZDOTDIR`에 보관한다.
  - Ghostty의 resources dir 탐색과 셸 감지에 의존하지 않는다.
  - 사용자 `.zshrc`는 통합 스크립트가 원래 `ZDOTDIR`을 복원한 뒤 평소처럼 로드된다.
  - `--clean-shell` 세션은 Block 기능의 대상이 아니다(Block 없이 일반 터미널로 동작).
- **Block 확정 시점:** `COMMAND_FINISHED`(OSC 133 D)를 받을 때 shim이 그 자리에서 직전 명령의 입력과 출력을 읽어 하나의 이벤트로 보낸다. prompt·start 이벤트는 MVP 2에서 추가하지 않는다. 시작 시각은 종료 시각에서 duration을 빼서 구한다.
- **vendor 노출:** `apprt/embedded.zig`에 export 세 개만 추가한다. 범용 semantic API는 만들지 않는다.
  - `ghostty_surface_read_last_command`: 직전 prompt의 input 셀
  - `ghostty_surface_read_last_output`: `selectOutput` 결과
  - `ghostty_surface_is_alternate_screen`
- **Block 표시:** 화면은 Ghostty native surface가 그린다. 따라서 Warp처럼 출력 사이에 Block을 끼워 넣는 inline 표시는 자체 렌더러 없이는 할 수 없다. MVP 2는 터미널 옆에 GPUI로 그린 **Block 목록 패널**에서 명령, 상태, 접기, 복사, 재실행을 제공한다. inline 표시는 자체 렌더러를 결정할 때 다시 검토한다.
- **불완전 표시:** 다음 경우 Block을 지우지 않고 `incomplete` 사유와 함께 남긴다.
  - 출력이 스크롤백 한도나 이벤트 한도를 넘음
  - `ObservationGap`
  - 명령 종료 시점에 Alternate Screen 상태
  - exit code가 없는 D

## 단계

| 단계 | 브랜치(예) | 목표 | 완료 기준 |
| --- | --- | --- | --- |
| B0 | `feat/mvp2-shell-integration` | 위 두 원인을 실행으로 확인하고, `ZDOTDIR` 주입과 번들 배포 | 일반 실행에서 명령마다 `CommandFinished`가 들어오고, 사용자 rc도 로드됨. clean shell과 통합 디렉터리가 없는 경우는 기존처럼 동작 |
| B1 | `vendor/command-boundary-api` | export 3개, shim 호출, 큐의 가변 길이 텍스트 | 명령·출력 텍스트가 한도 안에서 손실 없이 전달되고, 한도를 넘으면 truncated로 표시. vendor 테스트 추가, `BAREFRAME.md` 기록 |
| B2 | `feat/mvp2-block-model` | `bareframe-core`에 Block 모델 추가(GPUI 비의존) | 연속·실패·여러 줄·긴 출력·취소(130)·gap·Alternate Screen 상태 전이를 단위 테스트로 확인 |
| B3 | `feat/mvp2-block-panel` | Block 목록 패널과 접기·복사·재실행 | 재실행은 실행 중이 아닌 prompt에서만 명령 텍스트를 PTY에 보냄. Pane 포커스와 터미널 입력이 서로 간섭하지 않음 |
| B4 | `feat/mvp2-edge-cases` | TUI, SSH, 중첩 셸, 통합 없는 세션 | 원격 셸이나 hook이 없는 세션에서는 Block을 만들지 않고 터미널이 정상 동작 |
| B5 | `docs/mvp2-validation` | 수용 검증과 `docs/mvp-2-validation.md` | 로드맵 완료 기준 시나리오 PASS, 그 뒤 `mvp-2` 태그 |

B0이 실패하면 B1 이후의 범위를 다시 정한다. 그 경우의 대안은 bareframe 전용 zsh hook을 만들어 명령 문자열을 OSC로 직접 보내는 것이다.
B1은 vendor만 변경하는 PR, B2는 core만 변경하는 PR로 나누어 서로 독립적으로 리뷰할 수 있게 한다.

## 수용 검증 시나리오

| 시나리오 | 기대 결과 |
| --- | --- |
| 연속 명령 3개 | 순서·명령·출력·exit code가 각 Block과 일치 |
| 실패 명령(`false`, 없는 명령) | exit code 1 / 127, 출력 보존 |
| 여러 줄 명령(`\` 이어 쓰기, heredoc) | 명령 텍스트 전체가 하나의 Block |
| 긴 출력(스크롤백 이상) | truncated로 표시. 앱은 멈추지 않음 |
| `sleep` 후 Ctrl+C | exit 130, 취소된 Block |
| vim·less·htop 실행 후 종료 | 터미널 정상. 해당 Block은 출력 없이 TUI 사용으로 표시 |
| ssh 접속과 원격 명령 | 원격 명령은 Block으로 만들지 않음. 종료 후 로컬 Block 재개 |
| `--clean-shell`, 사용자 `ZDOTDIR` 설정 | 각각 Block 없이 정상 / 사용자 설정이 로드되고 Block 동작 |
| Block 복사·접기·재실행 | 복사 내용 일치, 재실행 시 새 Block 생성 |

## 이월 항목

MVP 1 리뷰에서 나온 항목이다. 각각 별도 `fix/` PR로 처리하며, MVP 2 완료 조건에는 포함하지 않는다.

- **붙여넣기 보호:** `src/terminal/adapter.rs`의 `clipboard_approval`이 Ghostty의 위험한 붙여넣기 승인 요청을 모두 자동 승인한다. 그래서 bracketed paste가 꺼진 프로그램에서 pastejacking을 막지 못한다. 거부로 바꾸면 `cat`이나 원격 셸에서 여러 줄 붙여넣기가 막히므로, 내용을 보여 주는 확인 UI로 해결한다.
- **IME:**
  - `unmark_text`가 조합을 버린다. Cocoa의 관례는 확정이다.
  - 확정이 보관된 상태에서 Option·Ctrl+Backspace가 오면 그 키도 함께 버려진다.
  - 마우스 보고 바이트가 확정 텍스트보다 먼저 나갈 수 있다.
- **세션 표시:** 셸 실행 실패가 `Failed`가 아니라 세션 종료로 표시된다.
- **exit code:** Ghostty는 OSC 133 D에 코드가 없으면 0으로 처리한다(`termio/stream_handler.zig:1002-1010`). bareframe은 알 수 없는 값을 0으로 표시하지 않으므로, B1에서 이 경로를 구분할 수 있는지 확인한다.
