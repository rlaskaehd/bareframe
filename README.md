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

- `crates/bareframe-core`: GPUI에 의존하지 않는 모델. 탭·그룹(`workspace.rs`), 분할 레이아웃(`layout.rs`), 상태 저장, 설정·키맵, git 출력 해석, 텍스트 버퍼, IME 조합.
- `src/app.rs`: 앱 시작과 창. 액션·키 바인딩은 `actions.rs`, 메뉴바는 `menu_bar.rs`, 설정·상태 파일은 `files.rs`.
- `src/workspace.rs`: 창 하나의 뷰 상태와 Pane 수명. `src/workspace/` 아래에 사이드바, 분할 레이아웃, 드래그 앤 드롭(`drag.rs`), 사이드 열 크기(`columns.rs`), 액션 배선(`commands.rs`), 저장·복원(`persist.rs`), 설정 적용(`prefs.rs`) 등이 기능별로 있다.
- `src/terminal.rs`: GPUI 입력과 Ghostty 연결. native 자원은 이 계층이 관리한다.
- `src/editor/`, `src/markdown/`: 코드 편집기와 Markdown 뷰어. 그리기·찾기·파일·언어 서버·상태 줄·파싱을 모듈로 나눴다.
- `src/lsp/`: 언어 서버 관리. 서버 찾기(`install.rs`), 실행 중 서버와의 대화(`server.rs`), 프로세스(`client.rs`), Python 인터프리터(`python.rs`), 기억해 둔 선택(`store.rs`). 프로세스가 필요 없는 부분(서버 표, 프레이밍, URI)은 core의 `lsp.rs`.
- `src/widgets.rs`, `src/theme.rs`: 여러 패널이 쓰는 작은 위젯과 색 테마.
- `vendor/gpui-libghostty`: 고정된 upstream 사본과 macOS 통합 수정.
- `reference/`: 변경하지 않는 upstream 참조 자료.

MVP 2 계획은 [MVP 2 구현 계획](docs/mvp-2-plan.md)에 있다. 보류한 명령 Block 조사는 [명령 Block 확장 경계](docs/block-boundary.md)에 남겼다.

## 라이선스

bareframe는 [GNU Affero General Public License v3.0 이상](LICENSE)(`AGPL-3.0-or-later`)으로 배포한다.
상업적 사용을 포함해 누구나 쓰고 고칠 수 있지만, 수정본을 배포하거나 네트워크 서비스로 제공하면 전체 소스를 같은 라이선스로 공개해야 한다.

함께 배포하는 구성 요소는 각자의 라이선스를 따른다.
- `vendor/gpui-libghostty`: MIT([LICENSE](vendor/gpui-libghostty/LICENSE)). 그 안의 Ghostty: MIT.
- Ghostty zsh 셸 통합(`Contents/Resources/shell-integration`): GPL-3.0([resources/shell-integration](resources/shell-integration)).
- GPUI: Apache-2.0.
