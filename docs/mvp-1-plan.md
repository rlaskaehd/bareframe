# MVP 1 구현 계획

bareframe의 첫 구현 목표는 **macOS에서 단일 창과 단일 zsh 세션으로 사용할 수 있는 일반 터미널**이다.
GPUI는 앱 창과 입력 경로를, libghostty는 터미널 화면과 셸 세션을 담당하는 구성을 먼저 검증한다.
명령 Block과 여러 Pane은 후속 단계로 두고, 앱 코드에서는 터미널 통합 경계를 분리한다.

작성 기준일은 2026-10-26이다. 아래 단계는 모두 구현했으며, 결과는 [검증 기록](mvp-1-validation.md)을 따른다.

## 범위와 사용 흐름

앱을 실행하면 지정한 작업 디렉터리에서 zsh가 열린다.
명령을 입력하고 출력과 스크롤백을 확인하며, 텍스트 선택과 복사·붙여넣기를 사용할 수 있다.
`vim`, `less`, `htop`, `ssh`를 실행한 뒤 프롬프트로 돌아와 계속 작업할 수 있어야 한다.
리사이즈, 앱 포커스 전환, 한글 입력, 셸 종료와 창 닫기도 이 범위에 포함한다.

외형은 하나의 터미널과 시작 실패·세션 종료 상태를 표시하는 수준으로 시작한다.
Warp 스타일 Block, 탭, 화면 분할, Sidebar, 편집기, Git Diff, 브라우저, AI Agent, Workspace 영속화는 MVP 2 이후 범위다.

## 통합 방식

```text
bareframe 앱
├── AppShell                   창과 앱 수명, 최소 메뉴
├── WorkspaceView              단일 TerminalPane 배치
└── TerminalPane
    └── 내부 GhosttyAdapter    설정 변환, 입력, 상태 관찰, 해제
        └── gpui-libghostty    GPUI entity와 macOS native surface 연결
            └── libghostty     VT 상태, 폰트, Metal 렌더링, PTY와 셸 실행
```

**초기 구현에서는 Ghostty가 PTY와 셸을 관리하는 경로를 사용한다.**
`portable-pty`를 함께 연결해 두 번째 PTY를 생성하지 않는다.
GPUI task와 어댑터 wakeup을 먼저 사용하고, 별도 비동기 작업이 필요해질 때 Tokio를 검토한다.
전체 libghostty는 터미널 에뮬레이션·폰트·렌더링을 제공하므로 MVP 1에서 셀 렌더러를 직접 만들지 않는다.
참조: [Ghostty 공식 설명](https://ghostty.org/docs/about#libghostty),
[native surface 생성과 해제](../reference/gpui-libghostty/crates/gpui-ghostty/shim/ghostty_surface.m).

`GhosttyAdapter`는 프로젝트 내부 모듈 경계로 시작한다. 여러 backend를 위한 범용 trait이나 plugin 체계는 필요할 때 추가한다.
Block Engine용 lifecycle과 출력 경계에 접근할 지점은 조사해 기록하되, MVP 1에서 Block 저장과 렌더링을 구현하지 않는다.

## 의존성과 환경 기준

| 항목 | 초기 고정 후보와 처리 |
| --- | --- |
| 플랫폼 | macOS arm64 우선. 다른 플랫폼과 Intel 지원은 후속 범위 |
| Rust | GPUI 기준 소스의 toolchain인 1.95.0을 초기 재현 기준으로 검증. 프로젝트 `rust-toolchain.toml`에 고정 |
| GPUI / gpui_platform | 동일 Zed revision `cc053a4a6fa2fd0e8793201ed9099466af1be0b1`을 Git 의존성으로 사용 |
| gpui-libghostty | 확보한 revision `2bbde4ae53a8e163cdbe05294bd09934b30c1af3`을 초기 기준으로 사용 |
| Ghostty | 어댑터 안에 포함된 `9f0e1719dc918368367d368bfe300f59bb68b5a4` 기반 수정 소스 사용 |
| Zig | vendored 소스의 최소 요구인 0.16.0을 우선 검증하고 정확한 사용 버전 기록 |
| macOS 빌드 도구 | Xcode, clang, Metal compiler, SDK 경로 확인 |
| 잠금 파일 | 앱의 `Cargo.lock`을 추적. 참조 자료용 `sources.lock.json`과 구분 |

초기 의존성은 Cargo의 Git URL과 전체 revision으로 지정한다.
`reference/`는 무결성을 확인한 읽기용 스냅샷으로 유지하고, 앱의 path dependency나 수정 위치로 사용하지 않는다.
어댑터 보완이 필요하면 독립적인 `vendor/gpui-libghostty/` 사본과 변경 기록을 만들거나 고정된 Fork revision을 사용한다.
이때 포함된 Ghostty 수정분과 원래 라이선스 파일을 함께 보존한다.

로컬에서 확인한 Rust는 1.98.1이며, Zig와 `htop`은 현재 PATH에서 발견되지 않았다.
Xcode 경로, clang, Metal compiler와 SDK 조회는 가능했다.
M0에서 의존성 취득과 실제 컴파일을 확인한 뒤 사용할 도구 버전을 확정한다.
이는 프로젝트별 설정으로 처리하며 전역 Rust 기본 버전은 변경할 필요가 없다.

GPUI revision과 Rust 기준의 근거는 [어댑터 Cargo.toml](../reference/gpui-libghostty/Cargo.toml),
[GPUI upstream toolchain](https://github.com/zed-industries/zed/blob/cc053a4a6fa2fd0e8793201ed9099466af1be0b1/rust-toolchain.toml)이다.
Ghostty와 Zig 기준은 [VENDOR.md](../reference/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty/VENDOR.md),
[build.zig.zon](../reference/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty/build.zig.zon)을 따른다.

## 단계별 구현

| 단계 | 목표 | 완료 기준 |
| --- | --- | --- |
| M0 | 도구와 의존성 고정 | pinned source와 빌드 의존성을 취득하고 GPUI 및 native library 컴파일 성공 |
| M1 | 최소 터미널 구동 | GPUI 창에 zsh 프롬프트가 표시되고 키 입력과 출력이 왕복 |
| M2 | 앱 구조와 실행 설정 | 내부 어댑터 경계, 작업 디렉터리, 셸 설정, 기본 창·메뉴 동작 정리 |
| M3 | 입력과 클립보드 | 제어 키, 포커스, 선택·복사·붙여넣기, 한글 IME 검증 통과 |
| M4 | 화면과 크기 변경 | 스크롤백, Unicode, Alternate Screen, 리사이즈와 배율 변화 검증 통과 |
| M5 | 세션 수명과 오류 | 시작 실패·셸 종료·창 닫기 처리와 소유 자원 해제 검증 통과 |
| M6 | 수용 검증과 사용 문서 | 테스트 행렬의 필수 항목을 실행하고 실행·빌드·검증 절차 기록 |

M0부터 M2까지는 순서대로 진행한다. M3와 M4는 M2의 기반 위에서 진행하고, M5와 M6에서 전체 흐름을 검증한다.
M1에서 첫 프롬프트가 보이는 것을 중간 성과로 삼고, M6까지 통과했을 때 MVP 1을 완료한다.

### M0 환경과 의존성 고정

- Zig 실행 경로와 프로젝트 Rust toolchain을 준비한다.
- GPUI와 `gpui_platform`을 같은 revision으로 취득하고, 어댑터의 native build를 검증한다.
- Cargo registry, Git 의존성, Zig 패키지 다운로드에 필요한 네트워크를 확인한다. 기존 소스 ZIP만으로 모든 빌드 의존성이 확보된 것은 아니다.
- native·Zig build cache는 지원되는 환경 변수로 프로젝트의 `target/` 아래에 둘 수 있는지 확인한다.
- 실제 도구 버전, SDK, dependency revision, 컴파일 결과를 `docs/build.md`에 기록한다.

완료 기준은 실제 macOS 대상 컴파일 성공이다. 의존성 다운로드나 SDK 조회만 성공한 상태로 다음 단계가 준비되었다고 기록하지 않는다.

### M1 최소 터미널 구동

- 최소 GPUI 애플리케이션에서 `gpui_platform`으로 창을 연다.
- `gpui_libghostty::bind_gpui!`를 한 모듈에서 호출하고 `Terminal::spawn`으로 native surface를 연결한다.
- 기본 설정과 `/bin/zsh`로 시작해 `pwd`, `printf`, 간단한 연속 출력을 확인한다.
- surface의 위치·크기·포커스·wakeup과 첫 프레임을 확인한다.
- 한글 IME를 짧게 시험해 M3에서 필요한 수정의 범위를 일찍 파악한다.

native surface 표시나 입력 왕복이 실패하면 앱 기능을 추가하기 전에 이 단계에서 해결한다.
어댑터 수정으로 해결 가능한지 판단하고, VT API와 자체 렌더러로 변경해야 한다면 별도 설계와 일정으로 다룬다.

### M2 앱 구조와 실행 설정

- 단일 Cargo 패키지에서 창, WorkspaceView, TerminalPane, 통합 어댑터를 분리한다.
- `--cwd`를 지원하고 생략하면 앱 실행 디렉터리를 사용한다. 존재하지 않는 경로와 디렉터리가 아닌 경로는 명확한 오류로 처리한다.
- 기본 셸은 `/bin/zsh`의 대화형 로그인 실행을 검증한다. 테스트용으로 사용자 초기화 파일을 읽지 않는 clean shell 경로도 제공한다.
- 작업 디렉터리는 어댑터의 전용 필드로 전달한다. 경로를 셸 명령 문자열에 이어 붙이지 않는다.
- 초기 Ghostty 설정은 내장 기본값을 사용해 동작을 재현하고, 사용자 설정 자동 병합은 후속 기능으로 둔다.
- `Cmd+Q`, `Cmd+W`와 마지막 창 종료 동작을 정한다. 사용자의 `.zshrc`, `.zprofile`, Ghostty 설정 파일을 수정할 필요는 없다.

기존 shim은 `TERM=xterm-256color`, `COLORTERM=truecolor`를 설정한다.
MVP 1에서는 이 경로를 검증해 사용하고 `xterm-ghostty` terminfo 설치를 필수 조건으로 추가하지 않는다.

### M3 입력과 클립보드

- 문자, Return, Backspace, Delete, 방향키, Home/End, Tab, 키 반복, modifier 조합을 검증한다.
- `Ctrl+C`, `Ctrl+D`, `Ctrl+Z`, `fg`가 셸과 실행 중인 프로그램에 전달되는지 확인한다.
- 앱 메뉴 단축키와 터미널 키 전달이 중복 실행되지 않도록 입력 경로를 정리한다.
- 마우스 선택, 스크롤, `Cmd+C`·`Cmd+V`, 여러 줄 붙여넣기와 bracketed paste를 검증한다.
- 한글 IME의 조합 중 표시, 확정, 조합 취소, Backspace, 영문 전환을 검증한다. UTF-8 출력이 보이는 것과 조합 입력이 가능한 것은 별도 항목이다.

현재 어댑터의 [입력 처리](../reference/gpui-libghostty/crates/gpui-ghostty/src/adapter.rs)는 key down/up을 Ghostty로 전달한다.
명시적인 조합 입력 연결은 이 파일에서 확인되지 않으므로, GPUI text input과 Ghostty text/preedit API의 연결 필요성을 검증한다.
필요하면 유지 가능한 어댑터 사본에서 해당 연결을 보완한다. 조합 확정 문자와 key event가 중복 전송되지 않아야 한다.

### M4 화면과 리사이즈

- 창 크기 변경을 native surface와 PTY의 행·열 크기에 반영한다.
- `stty size`와 TUI 화면을 함께 확인해 화면만 늘어나고 PTY 크기는 남아 있는 문제를 잡는다.
- Retina 배율, 창 최소화·복원, 포커스 전환 뒤의 렌더링을 확인한다. 다른 배율의 모니터가 있으면 이동도 검증한다.
- ANSI 색상, 한글·CJK, combining character, 대표적인 emoji 출력과 커서 정렬을 확인한다.
- 긴 출력의 scrollback, 휠 스크롤, 선택 영역과 primary/alternate screen 전환을 검증한다.
- 일반 렌더링에 프레임 snapshot을 반복 사용하지 않고 Ghostty의 native 렌더링과 wakeup을 유지한다.

### M5 세션 종료와 오류 처리

- 앱 상태를 `Starting`, `Running`, `Exited`, `Failed`로 구분한다.
- 셸이 끝나면 종료 상태와 다시 시작 동작을 제공한다. 어댑터가 종료 코드를 제공하지 않으면 알 수 없는 값을 `0`으로 표시하지 않는다.
- native 초기화 실패와 셸 실행 실패를 구분해 메시지와 복구 경로를 제공한다.
- 창 닫기와 앱 종료에서 task, subscription, native surface, PTY 및 연결된 프로세스의 정리 순서를 검증한다.
- AppKit view와 surface는 UI thread에서 생성·해제하고, surface가 부모 창보다 먼저 정리되도록 소유 관계를 유지한다.
- 종료 후 callback과 wakeup이 해제된 객체에 접근하지 않는지 확인한다.

현재 어댑터에는 `is_alive()`가 있지만 종료 UI, 정확한 exit code, 자식 프로세스 정리까지 보장하는 앱 계약은 별도로 구현·검증해야 한다.
프로세스 정리 검증은 테스트에서 시작한 셸과 연결된 자식 프로세스를 대상으로 하며, 별도로 분리한 daemon과 다른 앱의 프로세스를 대상으로 확대하지 않는다.

### M6 수용 검증과 문서

- 다음 행렬을 clean shell에서 검증한 뒤 일반 사용자 zsh 설정에서도 기본 흐름을 재확인한다.
- `docs/build.md`, `docs/usage.md`, `docs/mvp-1-validation.md`를 실제 실행 결과로 완성한다.
- native 창을 만드는 smoke 검증과 화면을 열지 않는 Rust 검증의 결과를 구분한다.
- 앱 코드에 `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, `cargo build --locked`를 적용한다.

## 수용 검증 행렬

| 영역 | 시나리오 | 기대 결과 |
| --- | --- | --- |
| 실행 | 기본 실행과 `--cwd`, 공백·한글이 있는 경로 | 프롬프트와 `pwd`가 지정한 경로를 사용 |
| 셸 | 명령 실행, 파이프, 리다이렉션, `sleep` 취소, job suspend/resume | 일반 zsh와 같은 입력·제어 동작 |
| 입력 | modifier, 반복 입력, Tab, 앱 전환 후 재입력 | 누락·중복·포커스 손실 없음 |
| 한글 | IME 조합·확정·취소와 영문 전환 | 조합 표시와 입력이 일치하고 중복 확정 없음 |
| 클립보드 | 선택 복사, 단일·여러 줄 붙여넣기 | 선택 내용과 입력이 일치하고 bracketed paste 동작 |
| 화면 | 색상·Unicode·긴 출력·스크롤백 | 출력과 커서가 맞고 이전 출력 탐색 가능 |
| 크기 | 창 리사이즈, `stty size`, TUI 재배치 | PTY 크기와 native surface가 함께 갱신 |
| vim | 임시 파일 열기·편집·저장·종료와 리사이즈 | 입력·저장 성공 후 프롬프트 정상 복원 |
| less | 긴 임시 파일의 검색·스크롤·종료 | 화면과 키 처리가 정상이며 종료 후 복원 |
| htop | 실행·선택·리사이즈·종료 | 전체 화면 갱신과 입력이 정상 |
| ssh | 테스트 가능한 호스트 접속, 원격 명령·TUI·리사이즈·종료 | 원격 사용 후 로컬 프롬프트로 정상 복귀 |
| 종료 | `exit`, 빈 프롬프트의 `Ctrl+D`, 실행 중 창 닫기, `Cmd+Q` | 상태 전환과 소유 자원 정리, 앱 crash 없음 |
| 반복 | 세션 시작·종료 20회와 긴 출력 후 재입력 | 누적된 세션 프로세스와 멈춘 surface 없음 |
| 오류 | 잘못된 cwd, 실행할 수 없는 셸, native 초기화 실패 | 오류를 확인하고 다시 실행할 수 있음 |

`htop` 실행 파일과 SSH 테스트 호스트는 수용 검증 전에 준비한다. 시스템의 Remote Login 설정 변경을 기본 준비 절차로 삼지 않는다.
실행 가능한 SSH 대상이 없으면 그 항목은 `NOT_RUN`으로 남기며, 통과로 대체하지 않는다.
외부 모니터 등 추가 하드웨어가 없는 경우에는 해당 환경과 미검증 항목을 명시한다.

자동 테스트는 cwd·설정 검증, 구현한 입력 변환, 세션 상태 전이처럼 앱이 소유하는 동작에 집중한다.
Ghostty의 VT parser를 재검증하거나 화면 미실행 테스트만으로 TUI·IME 호환성을 판단하지 않는다.

## 예정 코드 구조

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
src/
├── main.rs
├── lib.rs
├── app.rs
├── workspace.rs
└── terminal/
    ├── mod.rs
    ├── adapter.rs
    ├── config.rs
    └── session.rs
docs/
├── build.md
├── usage.md
└── mvp-1-validation.md
```

`workspace.rs`는 단일 Pane을 배치하는 최소 root view다.
`adapter.rs`에서 GPUI binding과 외부 라이브러리 의존을 모으고, `session.rs`는 앱에서 관찰하는 상태와 수명을 관리한다.
`session.rs`에 별도의 PTY 실행기를 만들지 않는다.
입력·테마 코드가 커지면 그때 모듈을 나누고, Block Engine을 위한 빈 crate를 미리 추가하지 않는다.

## 구현 위험과 진행 기준

| 항목 | 먼저 확인할 내용 | 대응 |
| --- | --- | --- |
| 빌드 조합 | pinned GPUI, 어댑터, Zig와 SDK의 native 컴파일 | M0에서 버전 고정과 재현 절차 확정 |
| IME | key event와 조합 입력 경로 | M1에서 시험하고 M3에서 연결 보완 |
| 종료 | `is_alive()`와 실제 프로세스 수명의 차이 | M5에서 close callback과 자원 해제 검증 |
| native view | 크기·배율·포커스 및 GPUI 요소와의 관계 | M1과 M4에서 최소 화면으로 검증 |
| 향후 Block | lifecycle과 출력에 접근할 지점 | MVP 1 중 조사 결과를 기록하고 MVP 2 설계에 반영 |

1인 집중 개발을 가정한 초기 추정은 약 10–15 작업일이다. 이는 견적이며 빌드 완료 실측값이 아니다.
IME나 native shim 보완의 범위가 크면 5–10 작업일을 추가로 예상하고 M1 결과로 일정을 조정한다.

MVP 1 완료에는 재현 가능한 빌드, 기본 사용 흐름, 수용 행렬의 필수 시나리오 통과와 결과 기록이 필요하다.
기술 검증에서 확인하지 못한 항목은 `NOT_RUN` 또는 실패로 남겨 실제 지원 범위를 드러낸다.
