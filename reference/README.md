# 오픈소스 참조 소스

2026-10-26에 공식 GitHub 저장소의 기본 브랜치 커밋을 조회하고, **해당 커밋의 소스 ZIP**을 내려받아 아래 디렉터리에 풀었다.
현재 셸에서 GitHub DNS 조회가 실패하여 브라우저를 통해 다운로드했다.
각 디렉터리는 `.git`과 Git 이력이 없는 소스 스냅샷이다.

| 로컬 디렉터리 | 공식 저장소 | 용도 | 확인한 라이선스 표시 |
| --- | --- | --- | --- |
| [ghostty/](ghostty/) | [ghostty-org/ghostty](https://github.com/ghostty-org/ghostty) | 전체 libghostty, VT API, shell integration | MIT |
| [gpui-libghostty/](gpui-libghostty/) | [behzade/gpui-libghostty](https://github.com/behzade/gpui-libghostty) | GPUI에서 Ghostty surface를 임베드하는 초기 검증 | MIT |
| [zed/](zed/) | [zed-industries/zed](https://github.com/zed-industries/zed) | GPUI, native UI, Workspace와 Pane 구조 | GPUI: Apache-2.0; Zed: 주로 GPL-3.0-or-later, 표시된 구성 요소는 Apache-2.0 |
| [warp/](warp/) | [warpdotdev/warp](https://github.com/warpdotdev/warp) | 명령 lifecycle, Block 모델, Alternate Screen, UI 구조 | 클라이언트: AGPL-3.0-only; warpui / warpui_core: MIT |
| [wezterm/](wezterm/) | [wezterm/wezterm](https://github.com/wezterm/wezterm) | portable-pty의 세션·PTY 추상화 | portable-pty / WezTerm 코드: MIT |

라이선스 표는 해당 스냅샷의 README, LICENSE, Cargo.toml 표시를 기록한 것이다.
포함된 폰트·에셋·외부 코드에는 별도 표시가 있으므로 실제로 사용할 구성 요소의 원문을 함께 확인한다.
Warp 소스의 역할은 설계 참조이며, 애플리케이션 의존성으로 추가한 상태는 아니다.

## 먼저 살펴볼 경로

### gpui-libghostty — 첫 기술 검증

- [README.md](gpui-libghostty/README.md): 사용 예제와 도구 요구 사항, Alpha 상태.
- [Cargo.toml](gpui-libghostty/Cargo.toml): 검사에 사용하는 GPUI revision.
- [crates/gpui-ghostty/src/terminal.rs](gpui-libghostty/crates/gpui-ghostty/src/terminal.rs): `TerminalOptions`, 입력과 프레임 처리.
- [crates/gpui-ghostty/src/adapter.rs](gpui-libghostty/crates/gpui-ghostty/src/adapter.rs): GPUI binding.
- [crates/gpui-ghostty/shim/ghostty_surface.m](gpui-libghostty/crates/gpui-ghostty/shim/ghostty_surface.m): macOS native surface 연결.
- [crates/gpui-ghostty/vendor/ghostty/VENDOR.md](gpui-libghostty/crates/gpui-ghostty/vendor/ghostty/VENDOR.md): 포함한 Ghostty 커밋과 수정 사항.

이 저장소의 GPUI 고정 revision과 vendored Ghostty는 별도로 받은 `zed/`, `ghostty/`의 기본 브랜치 커밋과 다르다.
참조 소스 확보만으로 상호 빌드 호환성을 확인한 것은 아니다.

### Ghostty — 터미널과 셸 이벤트

- [include/ghostty.h](ghostty/include/ghostty.h): 전체 libghostty C API.
- [include/ghostty/vt.h](ghostty/include/ghostty/vt.h): VT API.
- [src/terminal/](ghostty/src/terminal/): 터미널 상태와 VT 처리.
- [src/termio/](ghostty/src/termio/): 터미널 입출력 처리.
- [src/shell-integration/zsh/ghostty-integration](ghostty/src/shell-integration/zsh/ghostty-integration): zsh hook과 OSC 133.

### Zed — GPUI와 Workspace

- [crates/gpui/](zed/crates/gpui/), [examples/](zed/crates/gpui/examples/): GPUI와 예제.
- [crates/gpui_macos/](zed/crates/gpui_macos/): macOS 플랫폼 구현.
- [crates/workspace/](zed/crates/workspace/): Workspace와 Pane 구조.
- [crates/terminal/](zed/crates/terminal/): Zed의 터미널 통합 구조 참조. bareframe의 터미널 엔진 선택과는 별개다.

### Warp — Block과 Alternate Screen

- [app/src/terminal/model/blocks.rs](warp/app/src/terminal/model/blocks.rs): `BlockList` 모델.
- [app/src/terminal/model/alt_screen.rs](warp/app/src/terminal/model/alt_screen.rs): Alternate Screen 모델.
- [app/src/terminal/block_list_element.rs](warp/app/src/terminal/block_list_element.rs): Block UI.
- [app/assets/bundled/bootstrap/zsh_body.sh](warp/app/assets/bundled/bootstrap/zsh_body.sh): 셸 lifecycle hook.
- [crates/warp_terminal/src/model/ansi/dcs_hooks.rs](warp/crates/warp_terminal/src/model/ansi/dcs_hooks.rs): 자체 hook 프로토콜.
- [crates/warpui/](warp/crates/warpui/), [crates/warpui_core/](warp/crates/warpui_core/): UI framework.

### WezTerm — portable-pty

- [pty/Cargo.toml](wezterm/pty/Cargo.toml): `portable-pty` 패키지 정의.
- [pty/src/lib.rs](wezterm/pty/src/lib.rs): PTY와 자식 프로세스 인터페이스.
- [pty/src/unix.rs](wezterm/pty/src/unix.rs): Unix 구현.
- [pty/examples/](wezterm/pty/examples/): 세션 실행 예제.

## 버전과 검증 범위

[sources.lock.json](sources.lock.json)에 공식 URL, 다운로드 당시 브랜치, 전체 커밋 SHA, ZIP SHA-256, 파일 수와 검증 방법을 기록했다.
압축 파일에 기록된 커밋을 GitHub ref 조회 결과와 대조하고, 전체 CRC와 추출 후 파일 내용, 심볼릭 링크 대상, 실행 권한을 확인했다.
스냅샷은 다운로드 시점의 참조 자료이며, 애플리케이션의 의존성 버전 잠금 파일은 아니다.

Git 이력, submodule 내용 및 빌드 시 추가 다운로드되는 의존성은 확보 범위에 포함하지 않았다.
특히 WezTerm의 `.gitmodules`에 명시된 네이티브 라이브러리는 별도 취득이 필요하다.
이번 작업에서는 upstream 빌드 스크립트, 패키지 설치, 빌드와 테스트를 실행하지 않았다.

각 소스 디렉터리는 [이 디렉터리의 .gitignore](.gitignore)에 등록했다.
추후 bareframe을 Git으로 관리할 때 참조 소스 전체가 함께 추가되는 것을 방지하며, 안내 문서와 lock 파일은 추적할 수 있다.

## 후속 의존성 후보

Tokio, serde, git2 등은 애플리케이션 요구가 정해진 뒤 Cargo 의존성으로 검토한다.
이번에는 소스 내부를 직접 살펴볼 필요가 큰 위 5개 저장소를 확보했다.
WKWebView는 macOS 플랫폼 API이며, Rust binding의 선택은 브라우저 Pane 단계에서 정한다.

## 공식 읽을거리

- [Ghostty: libghostty](https://ghostty.org/docs/about#libghostty)
- [Warp: The Block Model Behind Warp's Agentic Development Environment](https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment)
- [gpui-libghostty README](https://github.com/behzade/gpui-libghostty#readme)
