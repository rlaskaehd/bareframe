# bareframe

Rust + GPUI + libghostty 기반의 모듈형 개발 워크스페이스 터미널.
macOS arm64용 MVP 1(단일 창·단일 zsh 터미널)을 완료했고, MVP 2(Warp 스타일 UI: 탭·분할, 테마·타이포그래피)를 구현 중이다.

## 개발 실행

```sh
./script/bootstrap
./script/run --clean-shell
./script/run --cwd ~/my-project
```

`bootstrap`은 Zig 0.16.0과 Cargo 의존성을 준비하고 앱을 빌드한다.
도구는 `.tools/`, Cargo 캐시는 `.cache/`, 빌드 산출물은 `target/`을 기본 경로로 사용한다.
Rust 1.98.1은 `rust-toolchain.toml`로 선택하며 전역 기본 toolchain을 변경하지 않는다.

## 검증

```sh
./script/check-core  # UI 의존성 없이 모델 테스트와 native shim 검사
./script/check       # 의존성 준비 후 전체 Rust 검사와 빌드
```

현재 실행한 검증과 남은 항목은 [MVP 1 검증 기록](docs/mvp-1-validation.md)에 구분해 기록한다.
실행 방법은 [사용 안내](docs/usage.md), 의존성은 [빌드 안내](docs/build.md)를 참조한다.

## 구조

- `crates/bareframe-core`: GPUI에 의존하지 않는 세션·Pane 식별자, 상태, 이벤트, 설정, IME 조합 모델.
- `src/workspace.rs`: Workspace 상태와 터미널 Pane의 수명 관리.
- `src/terminal/adapter.rs`: GPUI 입력과 Ghostty 연결. native 자원은 이 계층이 관리한다.
- `vendor/gpui-libghostty`: 고정된 upstream 사본과 macOS 통합 수정.
- `reference/`: 변경하지 않는 upstream 참조 자료.

MVP 2 계획은 [MVP 2 구현 계획](docs/mvp-2-plan.md)에 있다. 보류한 명령 Block 조사는 [명령 Block 확장 경계](docs/block-boundary.md)에 남겼다.
