# MVP 2 구현 계획 — 명령 Block

기준: `main`의 `mvp-1` 태그(2026-10-26). 범위와 완료 기준은 [로드맵](roadmap.md#mvp-2--명령-block)을, 이벤트 경계는 [MVP 2 확장 경계](mvp-2-boundary.md)를 따른다.
작업 방식은 [Git 전략](git-strategy.md)을 따른다.

## 현재 상태 조사 결과

vendored Ghostty(`vendor/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty`)를 조사한 결과다. 경로는 그 아래 `src/`를 기준으로 한다.

| 항목 | 상태 | 근거 |
| --- | --- | --- |
| OSC 133 파싱(A/B/C/D, exit code) | 있음 | `terminal/osc/parsers/semantic_prompt.zig`, `termio/stream_handler.zig:993-1027` |
| 행·셀 semantic 표시(prompt/input/output) | 있음. 내부 전용 | `terminal/page.zig:1985`, `:2094` |
| 직전 명령 출력 선택 | 있음. 내부 전용 | `Screen.selectOutput`, `PageList.promptIterator` |
| 커서가 프롬프트에 있는지 확인 | 있음. 내부 전용 | `terminal/Terminal.zig:2240` `cursorIsAtPrompt` |
| 앱으로 오는 명령 이벤트 | `COMMAND_FINISHED`(exit code, duration)만 있음. 메일박스를 거쳐 비동기로 전달 | `termio/stream_handler.zig:1002-1011`, `Surface.zig:1145-1170` |
| 명령 문자열 | 없음. zsh 스크립트의 `133;C`에 문자열이 들어 있지 않음 | `shell-integration/zsh/ghostty-integration:229` |
| Alternate Screen 알림·getter | 없음 | `Terminal.zig:4783`, C API 목록 |
| 텍스트 읽기 C API | 보이는 화면만 읽음. 스크롤백 구간은 지정할 수 없고, 비용이 큰 작업으로 문서화됨 | `apprt/embedded.zig:1381`, `:1660` |
| bareframe의 셸 통합 | **꺼져 있음** | 아래 실행 결과 |

`target/debug/bareframe --cwd /tmp`를 두 번 실행해 Ghostty stderr 로그로 확인했다(Reviewer, 2026-10-26).

| 실행 | 환경 | 로그 |
| --- | --- | --- |
| A | Ghostty 관련 환경 변수 상속을 지움 | `no resources dir set, shell integration disabled` (`termio/Exec.zig:799`) |
| B | cmux 셸 환경을 그대로 상속(`GHOSTTY_RESOURCES_DIR`) | `found Ghostty resources dir` 다음 `shell could not be detected` |

1. **resources dir을 찾지 못한다.** Ghostty는 resources dir을 두 경로로 찾는다(`os/resourcesdir.zig:51-104`).
   - 앱 프로세스의 `GHOSTTY_RESOURCES_DIR` 환경 변수
   - 실행 파일 위쪽 경로의 `Contents/Resources/terminfo/78/xterm-ghostty` 표식 파일

   bareframe 번들에는 표식 파일이 없다. 그래서 cmux나 Ghostty 안에서 `./script/run`으로 실행하면 상속된 값으로 resources dir이 잡히고, `open`으로 실행하면 잡히지 않는다. 결과가 실행 환경에 따라 달라진다.
2. **셸 감지가 실패한다.** `config.rs`의 `command()`는 셸 경로를 `'/bin/zsh'`처럼 작은따옴표로 감싼다. 그런데 Ghostty의 인자 분리기(Zig std, `single_quotes = false`)는 작은따옴표를 따옴표로 처리하지 않는다. 그래서 basename이 `zsh'`가 되어 zsh로 감지되지 않는다(`config/command.zig:126`, `termio/shell_integration.zig:141-164`).

`--clean-shell`(`zsh -f`)은 `.zshenv`를 읽지 않으므로, 이 방식으로는 셸 통합을 켤 수 없다.

## 설계 결정

### 셸 통합 주입

- Ghostty의 자체 주입은 끈다. shim이 만드는 config에 `shell-integration = none`을 넣는다(`termio/Exec.zig:782-789`).
  - 그러면 resources dir과 셸 감지, 상속된 다른 버전의 스크립트와 무관해진다.
  - Ghostty가 resources dir을 찾을 때 함께 설정하는 `TERMINFO` 등의 상속 문제도 B0에서 함께 확인한다.
- shim이 surface 환경 변수에 `ZDOTDIR`을 넣는다. 값은 앱이 배포한 vendored zsh 통합 디렉터리다.
  - 원래 `ZDOTDIR`이 설정되어 있을 때만 `GHOSTTY_ZSH_ZDOTDIR`에 보관한다. 빈 값을 넣으면 `.zshenv`가 `ZDOTDIR=""`로 복원한다.
  - 사용자 `.zshrc`는 통합 스크립트가 원래 `ZDOTDIR`을 복원한 뒤 평소처럼 로드된다.
- `shell-integration-features`는 `no-cursor,no-title,no-path`로 정해 MVP 1의 화면과 `PATH`를 유지한다. 이렇게 해도 OSC 7(cwd)과 OSC 133은 그대로 나온다.
- `.zshenv`와 `ghostty-integration`은 Kitty에서 온 GPLv3 파일이다. 번들에 넣을 때 라이선스 표시를 함께 넣는다.
- `--clean-shell` 세션은 Block 기능 대상이 아니다. Block 없이 일반 터미널로 동작한다.

### 명령 경계 스냅샷

- **D와 원자적으로 읽는다.** 앱 스레드가 `COMMAND_FINISHED`를 처리하는 시점에는 zsh가 이미 다음 프롬프트를 그린 뒤다. 그때 읽은 "직전 prompt"는 새 빈 프롬프트다.
  - 그래서 IO 스레드가 D를 처리할 때, 즉 터미널 lock을 잡은 상태에서 명령 입력과 출력을 읽어 메시지에 실어 보낸다.
  - 이 방식이 막히면 D 위치의 tracked pin을 함께 넘기는 방법을 쓴다.
  - export 개수는 고정하지 않는다. 이 요구를 만족하는 가장 작은 vendor 변경을 고른다.
- **읽기 상한.** 출력은 행 수와 바이트 수 상한 안에서만 읽는다. 상한을 넘으면 `truncated`로 표시한다.
- **시작 시각.** 시작 시각은 종료 시각에서 duration을 빼서 구한다.
- **cwd.** Block의 cwd는 명령을 시작할 때의 위치, 즉 직전 D 이후 마지막으로 받은 OSC 7 값이다. 명령 안에서 `cd`로 바뀐 값은 다음 Block의 cwd가 된다.
- **TUI 사용.** TUI는 종료하기 전에 primary 화면으로 돌아오므로, D 시점의 Alternate Screen 상태로는 판정할 수 없다.
  - 그래서 "직전 D 이후 Alternate Screen에 들어간 적이 있는가"를 sticky 플래그로 기록하고 D에서 초기화한다.
  - 1차 방법은 vendor에서 화면 전환 시 플래그를 세우는 것이다. 막히면 getter를 tick마다 확인한다.
- **재실행 가능 여부.** `cursorIsAtPrompt` getter를 노출해 판단한다. 커서가 프롬프트에 없거나 입력 중인 글자가 남아 있으면 재실행을 거부하고 이유를 표시한다.

### 표시와 불완전 처리

- **Block 표시.** 화면은 Ghostty native surface가 그린다. Warp처럼 출력 사이에 Block을 끼워 넣으려면 자체 렌더러가 있어야 한다.
  - 그래서 MVP 2는 터미널 옆에 GPUI로 그린 **Block 목록 패널**에서 명령, 상태, 접기, 복사, 재실행을 제공한다.
  - inline 표시는 자체 렌더러를 결정할 때 다시 검토한다.
- **불완전 표시.** 다음 경우에는 Block을 지우지 않고 `incomplete` 사유와 함께 남긴다.
  - 출력이 상한을 넘음
  - `ObservationGap`
  - 이벤트 순서가 맞지 않음

## 단계

| 단계 | 브랜치(예) | 목표 | 완료 기준 |
| --- | --- | --- | --- |
| B0 | `feat/mvp2-shell-integration` | `shell-integration = none`, `ZDOTDIR` 주입, 번들 배포와 라이선스 표시, validation instance의 TerminalEvent trace 파일 | 일반 실행의 trace에 명령마다 `CommandFinished`가 기록되고 사용자 rc가 로드됨. `open` 실행과 cmux의 `./script/run` 실행 결과가 같음. clean shell과 통합 디렉터리가 없는 경우는 기존처럼 동작 |
| B1 | `vendor/command-boundary-api` | D와 원자적인 명령·출력 스냅샷, sticky Alternate Screen 플래그, `cursorIsAtPrompt` getter, 큐의 가변 길이 텍스트 | 붙여넣은 여러 명령을 연속 실행해도 각 이벤트의 명령·출력이 해당 명령과 일치함. 상한을 넘으면 `truncated`. vendor 테스트와 `BAREFRAME.md` 기록 |
| B2 | `feat/mvp2-block-model` | `bareframe-core`의 Block 모델(GPUI 비의존) | 연속·실패·여러 줄·긴 출력·취소(130)·gap·TUI 사용·cwd 귀속의 상태 전이를 단위 테스트로 확인 |
| B3 | `feat/mvp2-block-panel` | Block 목록 패널과 접기·복사·재실행 | 재실행은 위 조건을 지키고 새 Block을 만듦. Pane 포커스와 터미널 입력이 서로 간섭하지 않음 |
| B4 | `feat/mvp2-edge-cases` | TUI, SSH, 중첩 셸, 통합 없는 세션, 사용자 프롬프트 프레임워크 | 원격 셸이나 hook이 없는 세션에서는 Block 없이 터미널이 정상 동작. precmd에서 출력하는 프레임워크(pure 등)에서는 귀속 오류를 기록하거나 막음 |
| B5 | `docs/mvp2-validation` | 수용 검증과 `docs/mvp-2-validation.md` | 로드맵 완료 기준 시나리오 PASS. 그 뒤 `mvp-2` 태그 |

B0이 실패하면 B1 이후 범위를 다시 정한다. 대안은 bareframe 전용 zsh hook이 명령 문자열을 OSC로 직접 보내는 것이다.
B1은 vendor 단독 PR, B2는 core 단독 PR로 나누어 서로 독립적으로 리뷰할 수 있게 한다.

## 수용 검증 시나리오

| 시나리오 | 기대 결과 |
| --- | --- |
| 연속 명령 3개, 여러 명령을 한 번에 붙여넣기 | 순서·명령·출력·exit code가 각 Block과 일치 |
| 실패 명령(`false`, 없는 명령) | exit code 1 / 127, 출력 보존 |
| 여러 줄 명령(`\` 이어 쓰기, heredoc) | 명령 텍스트 전체가 하나의 Block |
| 긴 출력(상한 이상) | `truncated` 표시. 앱이 멈추지 않음 |
| `sleep` 후 Ctrl+C | exit 130, 취소된 Block |
| vim·less·htop 실행 후 종료 | 터미널 정상. 해당 Block에 TUI 사용 표시 |
| `cd` 포함 명령 | Block cwd는 시작 위치, 다음 Block은 바뀐 위치 |
| ssh 접속과 원격 명령 | 원격 명령은 Block이 되지 않음. 종료 후 로컬 Block 재개 |
| `--clean-shell`, 사용자 `ZDOTDIR` 설정 | 각각 Block 없이 정상 / 사용자 설정이 로드되고 Block 동작 |
| Block 복사·접기·재실행, 실행 중 재실행 시도 | 복사 내용 일치, 재실행 시 새 Block, 실행 중이면 거부 |

## 이월 항목

MVP 1 리뷰에서 나온 항목이다. 각각 별도 `fix/`나 `chore/` PR로 처리하며, MVP 2 완료 조건에는 포함하지 않는다.

- **붙여넣기 보호.** `src/terminal/adapter.rs`의 `clipboard_approval`이 위험한 붙여넣기 승인 요청을 모두 자동 승인한다. 그래서 bracketed paste가 꺼진 프로그램에서 pastejacking을 막지 못한다.
  - 요청을 거부하도록 바꾸면 `cat`이나 원격 셸에서 여러 줄 붙여넣기가 막힌다.
  - 그래서 붙여넣을 내용을 보여 주는 확인 UI로 해결한다.
- **IME.**
  - `unmark_text`가 조합 중인 글자를 버린다. Cocoa의 관례는 확정이다.
  - 확정이 보관된 상태에서 Option·Ctrl+Backspace를 누르면 그 키도 함께 버려진다.
  - 마우스 보고 바이트가 확정 텍스트보다 먼저 나갈 수 있다.
- **세션 표시.** 셸 실행에 실패해도 `Failed`가 아니라 세션 종료로 표시된다.
- **GUI 검증 도구.** `target/validation/run2/tools/`(`keypost.swift` 등)는 추적되지 않는다. Reviewer가 재현할 수 있도록 `tests/` 아래로 옮긴다.
- **exit code(우선순위 낮음).** Ghostty는 코드 없는 D를 0으로 처리한다. 다만 vendored zsh 스크립트에서 코드 없는 D는 C 없이 Enter만 친 경우에만 나간다. 이때는 `COMMAND_FINISHED`도 나가지 않으므로 지금 경로에서는 생기지 않는다.
