# 아키텍처와 기술 선택

> 명령 Block과 Block Engine은 현재 로드맵에서 보류했다([로드맵](roadmap.md#보류--명령-block)). 아래의 Block 관련 내용은 후속 검토용이다.

## 목표

터미널을 중심으로 여러 개발 도구를 하나의 Workspace에 배치한다.
명령과 결과를 독립적인 Block으로 다루면서도 `vim`, `less`, `htop`, `ssh` 같은 일반 터미널 사용을 지원한다.

## 책임 분리

```text
Rust Application / GPUI
├── Workspace Core
│   ├── Window / Tab Manager
│   ├── Pane Layout Manager
│   └── Workspace State / Persistence
├── Terminal Core
│   ├── Session / PTY의 수명과 입출력
│   ├── libghostty Adapter
│   └── Terminal Renderer / Alternate Screen
├── Block Engine
│   ├── Shell Integration
│   ├── Command Lifecycle
│   ├── Block Storage
│   └── Block Renderer
└── Workspace Extensions
    ├── Code Editor
    ├── Git Diff Viewer
    ├── File Explorer
    ├── Browser
    └── AI Agent (후속 확장)
```

Workspace Core는 Pane의 배치와 저장 가능한 상태를 관리한다. 터미널 세션의 내부 구현에 의존하지 않는다.
Terminal Core는 세션, 터미널 화면 상태, 키보드·마우스 입력과 리사이즈를 담당한다.
Block Engine은 명령의 시작·종료·종료 코드·작업 디렉터리와 출력의 대응 관계를 관리한다.

셸과 프로그램의 출력은 세션에서 터미널 엔진으로 전달한다.
Shell Integration에서 얻은 명령 이벤트는 어댑터를 통해 Block Engine에도 전달한다.
이벤트를 얻는 지점과 명령별 출력을 보존하는 방법은 기술 검증에서 결정한다.

## 명령 Block

- 명령 문자열, 식별자, 실행 상태, 종료 코드, 작업 디렉터리, 시작·종료 시각을 모델에 포함한다.
- 출력은 ANSI/VT 상태를 고려해 명령과 연결한다. 문자열을 줄 단위로 나누는 방법만으로 경계를 추정하지 않는다.
- zsh의 `preexec` / `precmd`와 OSC 133을 우선 검토한다. 명령 문자열과 추가 메타데이터의 전달 규약은 별도로 정한다.
- Alternate Screen 활성화 여부는 터미널 엔진 상태를 따른다. 전체 화면 TUI의 화면 갱신을 일반 출력 Block으로 누적하지 않는다.
- Shell Integration이 없는 원격 셸 등에서는 일반 터미널로 동작하는 경로를 유지한다.

Warp의 구현은 참고할 수 있지만, Warp가 OSC 133만으로 Block을 구현한다고 가정하지 않는다.
확보한 소스에는 JSON payload를 운반하는 자체 DCS hook 규약과 `DProtoHook` 모델이 있다.
참조: [Warp Block Model 설명](https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment),
[로컬 hook 모델](../reference/warp/crates/warp_terminal/src/model/ansi/dcs_hooks.rs).

## 기술 후보

| 영역 | 초기 선택 / 후보 | 결정할 사항 |
| --- | --- | --- |
| 언어 | Rust | GPUI 및 어댑터가 요구하는 toolchain 고정 |
| 데스크톱 UI | GPUI | 어댑터와 호환되는 정확한 Zed/GPUI revision |
| 터미널 | libghostty + 내부 어댑터 | 전체 surface API 또는 VT API의 사용 범위 |
| 통합 검증 | gpui-libghostty | native surface 배치, 포커스, 입력, 리사이즈 |
| 셸 / PTY | MVP 1은 Ghostty 세션 경로 우선 | 실제 수명 관리 검증, portable-pty는 별도 필요가 생길 때 검토 |
| 비동기 작업 | MVP 1은 GPUI executor 우선 | 별도 비동기 작업이 필요할 때 Tokio 검토 |
| 명령 구분 | zsh hook + OSC 133 후보 | 명령 텍스트, 종료 코드, 출력 경계 전달 |
| Workspace 상태 | Rust 모델 + 직렬화 | 저장 포맷과 복원 가능한 상태 범위 |
| Git | Git CLI 또는 git2 | 초기 요구에 맞춰 선택 |
| 브라우저 | macOS WKWebView 어댑터 후보 | GPUI Pane 안의 native view 통합 |

## 초기 통합에서 확인할 사항

**전체 libghostty와 libghostty-vt는 제공하는 책임이 다르다.**
전체 libghostty는 터미널 에뮬레이션, 폰트와 렌더링을 포함하고, VT API는 터미널 상태 처리를 위한 별도 API다.
전체 코어의 안정적인 독립 API를 전제로 설계하지 않고, 애플리케이션 안에 교체 가능한 어댑터 경계를 둔다.
참조: [Ghostty 공식 설명](https://ghostty.org/docs/about#libghostty),
[전체 헤더](../reference/ghostty/include/ghostty.h), [VT 헤더](../reference/ghostty/include/ghostty/vt.h).

확보한 `gpui-libghostty`는 Alpha 상태이며 README에 Rust 1.95와 Zig 0.16을 요구한다.
테스트와 예제의 GPUI 의존성은 Zed의 `cc053a4a6fa2fd0e8793201ed9099466af1be0b1`로 고정되어 있다.
이는 참조용으로 별도 확보한 Zed `main` 스냅샷과 다르므로 두 최신 저장소의 조합이 검증되었다고 간주하지 않는다.
참조: [어댑터 README](../reference/gpui-libghostty/README.md),
[GPUI 의존성](../reference/gpui-libghostty/Cargo.toml).

어댑터 내부 Ghostty는 `9f0e1719dc918368367d368bfe300f59bb68b5a4`를 기반으로 필요한 소스와 수정 사항을 포함한다.
별도 확보한 Ghostty `main`을 그대로 대체해 빌드할 수 있다고 가정하지 않는다.
참조: [vendored Ghostty 기록](../reference/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty/VENDOR.md).

어댑터의 macOS shim은 Ghostty surface를 만들고 실행할 명령과 작업 디렉터리를 전달한다.
이 경로의 PTY·셸 수명 관리와 명령 이벤트 노출 방식을 먼저 확인한 뒤 `portable-pty` 추가 여부를 정한다.
이는 세션을 두 계층에서 중복 생성하지 않기 위한 결정이다.
참조: [native surface shim](../reference/gpui-libghostty/crates/gpui-ghostty/shim/ghostty_surface.m).

MVP 1의 구체적인 통합 기준과 검증 순서는 [MVP 1 구현 계획](mvp-1-plan.md)을 따른다.

## Decision

### Context

터미널에서 시작해 편집기와 브라우저를 포함하는 Workspace로 확장할 수 있어야 한다.

### Options

- Warp Fork: 기존 Block과 UI 구조를 바탕으로 확장한다.
- GPUI + libghostty: 터미널 코어를 재사용하고 Workspace와 Block 모델을 직접 구성한다.
- WarpUI + libghostty: Warp의 UI framework를 사용하고 터미널 통합 계층을 구성한다.

### Decision / Reason

GPUI + libghostty 기반 독립 애플리케이션을 우선 검증한다.
핵심 개발 역량을 Workspace와 Block Engine에 집중하고, UI와 터미널 구현의 경계를 직접 관리하기 위해서다.

### Consequences

명령 lifecycle, 출력 보존, Block 가상화, Workspace 저장·복원은 직접 구현해야 한다.
초기 기술 검증에서 입력·렌더링 호환성과 명령 이벤트 접근 가능성을 확인하고 그 결과로 세부 설계를 확정한다.
