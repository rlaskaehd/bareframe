# 개발 빌드

대상은 macOS 14 이상의 Apple Silicon(arm64)이다. Rust 1.98.1(`rust-toolchain.toml`), Zig 0.16.0,
Xcode의 clang과 Metal compiler, 브라우저 Pane의 CEF를 빌드할 cmake와 ninja(`brew install cmake ninja`)를 사용한다.

## 준비와 빌드

```sh
./script/bootstrap
```

스크립트는 Xcode 도구를 확인하고 필요하면 Zig 0.16.0을 `.tools/`에 푼다.
Zig macOS arm64 압축 파일의 SHA-256은 [공식 다운로드 인덱스](https://ziglang.org/download/index.json)의 값
`b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489`와 대조한다.
기존 Zig를 쓰려면 `ZIG`에 실행 파일을 지정할 수 있으며 버전이 다르면 스크립트가 중단된다.
CEF 바이너리 배포본(약 135MB 다운로드, 풀면 약 340MB)은 `cef` crate와 같은 버전을 `.cache/cef`에 받는다
(`CEF_PATH`로 바꿀 수 있다).

브라우저 Pane은 앱 번들 안에서만 동작한다. `./script/bundle`이 `Contents/Frameworks`에 CEF 프레임워크와
helper 앱 다섯 개(`bareframe Helper`, `(GPU)`, `(Renderer)`, `(Plugin)`, `(Alerts)`, 모두 `bareframe-helper`
실행 파일)를 넣고 `codesign --deep`으로 다시 서명한다. 그래서 번들과 릴리스 zip이 100MB 넘게 커진다.
릴리스 빌드는 Chromium의 라이선스 고지(`CREDITS.html`)를 `Resources/CHROMIUM_CREDITS.html`로 싣는다.

Cargo 의존성을 해석해 최초 `Cargo.lock`을 생성한 뒤 `cargo build --locked`로 빌드한다.
이후 실행에서는 잠금 파일을 유지한다. 아직 의존성 취득이 끝나지 않은 환경에서는 앱의 lock 파일이 없을 수 있다.
코어 패키지는 외부 의존성이 없는 독립 manifest이므로 전체 GUI와 별도로 검증할 수 있다.

| 의존성 | 고정 기준 |
| --- | --- |
| GPUI / gpui_platform | Zed `cc053a4a6fa2fd0e8793201ed9099466af1be0b1` |
| gpui-libghostty | `vendor/`의 upstream `2bbde4ae53a8e163cdbe05294bd09934b30c1af3` 기반 사본 |
| Ghostty | 어댑터에 포함된 `9f0e1719dc918368367d368bfe300f59bb68b5a4` 기반 소스 |
| CEF | `cef` crate `=156.2.0`, 배포본 `156.2.0+156.0.14`(Chromium 156) |

어댑터 변경 내용은 [BAREFRAME.md](../vendor/gpui-libghostty/BAREFRAME.md)에 기록한다.

## 캐시 경로

프로젝트 스크립트는 Cargo 캐시를 `.cache/cargo`, 빌드 산출물을 `target/`으로 기본 설정한다.
`.cargo/config.toml`은 native library와 Zig 캐시 경로를 프로젝트 안으로 지정한다.
Zig의 추가 native 패키지는 어댑터 build script가 필요에 따라 다운로드한다.

`CARGO_HOME`, `CARGO_TARGET_DIR`, `ZIG`를 이미 지정했다면 스크립트는 해당 값을 사용한다.
캐시 폴더를 삭제하지 않고도 다시 빌드할 수 있으며, 전역 패키지 설치는 bootstrap의 작업에 포함하지 않는다.

## 검사 명령

```sh
./script/check-core
./script/check
```

첫 명령은 코어의 fmt·Clippy·테스트, C 이벤트 큐 실행 테스트, Objective-C shim 구문 검사를 수행한다.
두 번째 명령은 여기에 데스크톱 앱의 fmt·Clippy·테스트·빌드를 추가한다.
실제 창, 한글 IME, GPU 렌더링, TUI 호환성은 앱을 띄워 직접 확인한다.
`./script/bundle --validation`은 설정·상태 파일을 `target/validation/` 아래에 따로 두는 검증용 번들
(`dev.bareframe.validation`)을 만들어, 매일 쓰는 앱과 섞이지 않게 시험할 수 있다.
