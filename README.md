# bareframe

Rust + GPUI + libghostty로 만든 macOS용 개발 워크스페이스 터미널.

- Ghostty 터미널 엔진, 세로 탭 사이드바와 탭 그룹, 자유로운 분할, 맨 아래에서 시작하는 프롬프트
- 프로젝트 탐색기, 전역 검색, 커맨드 팔레트
- 코드 편집기(tree-sitter 문법 색, 언어 서버), Markdown 뷰어(Mermaid), Code review(헝크 되돌리기, 커밋)
- Settings 화면과 Ghostty 형식 설정 파일, 바꿀 수 있는 단축키, GitHub Releases 자동 업데이트

Apple Silicon Mac(macOS 14 이상)에서 동작한다.

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

## 배포용 번들

```sh
./script/bundle            # 매일 쓰는 디버그 번들: target/bareframe.app
./script/bundle --release  # 남에게 줄 최적화 번들: target/dist/bareframe.app
```

`--release` 번들은 Apple Silicon 전용이고, `LICENSE`와 내장한 모든 크레이트·아이콘의 라이선스를 모은
`THIRD_PARTY_NOTICES.txt`를 `Resources`에 넣는다. 그대로는 임시 서명(ad-hoc)이라 브라우저로 받은
사람은 처음 열 때 Finder에서 우클릭 › 열기를 해야 한다. 앱 아이콘은
`docs/brand/icon/bareframe-icon-1024.png`가 있을 때만 들어간다.

GitHub Release는 이렇게 만든다(업로드는 손으로).

```sh
./script/make-signing-identity   # 처음 한 번: 자체 서명 코드 서명 인증서를 로그인 키체인에
./script/release                 # target/dist/v<버전>/에 zip, .sha256, .sig
```

- 같은 인증서로 서명해야 업데이트 뒤에도 사용자가 준 개인정보 보호 권한이 유지된다.
- 앱의 업데이트 확인은 태그 `v<Cargo.toml의 version>`인 최신 릴리스에서
  `bareframe-<버전>-macos-arm64.zip`과 `.sig`를 찾고, 서명을 앱에 들어 있는 공개 키로 확인한다.
- 서명 키(`update-key`)와 인증서 사본은 `~/.config/bareframe-release`에만 있고 저장소에는
  넣지 않는다. 잃어버리면 이미 설치된 앱은 더는 자동 업데이트를 받지 못하므로 따로 백업한다.
- `bareframe-release`는 `cargo build`가 함께 만드는 관리자용 도구다(`keygen`, `sign FILE`, `notices OUT`).

## 문서

- [사용 안내](docs/usage.md): 실행, 화면 구성, Settings, 단축키, 업데이트.
- [빌드 안내](docs/build.md): 필요한 도구, 의존성 고정, 검사 명령.
- [구조](docs/architecture.md): 모듈 배치와 설계 원칙.
- [고치고 넓히기](docs/customizing.md): 설정·동작·색·언어를 더하는 곳, 포크 배포.

## 라이선스

bareframe는 [GNU Affero General Public License v3.0 이상](LICENSE)(`AGPL-3.0-or-later`)으로 배포한다.
상업적 사용을 포함해 누구나 쓰고 고칠 수 있지만, 수정본을 배포하거나 네트워크 서비스로 제공하면 전체 소스를 같은 라이선스로 공개해야 한다.

함께 배포하는 구성 요소는 각자의 라이선스를 따른다.
- `vendor/gpui-libghostty`: MIT([LICENSE](vendor/gpui-libghostty/LICENSE)). 그 안의 Ghostty: MIT.
- Ghostty zsh 셸 통합(`Contents/Resources/shell-integration`): GPL-3.0([resources/shell-integration](resources/shell-integration)).
- GPUI: Apache-2.0.
