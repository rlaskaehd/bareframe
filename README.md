# BareFrame

Rust + GPUI + libghostty로 만든 macOS용 개발 워크스페이스 터미널.

- Ghostty 터미널 엔진, 세로 탭 사이드바와 탭 그룹, 자유로운 분할, 맨 아래에서 시작하는 프롬프트
- 프로젝트 탐색기, 전역 검색, 커맨드 팔레트
- 코드 편집기(tree-sitter 문법 색, 언어 서버), Markdown 뷰어(Mermaid), Code review(헝크 되돌리기, 커밋)
- Settings 화면과 Ghostty 형식 설정 파일, 바꿀 수 있는 단축키, GitHub Releases 자동 업데이트

Apple Silicon Mac(macOS 14 이상)에서 동작한다.

## 설치

```sh
curl -fsSL https://raw.githubusercontent.com/rlaskaehd/bareframe/main/install.sh | sh
```

최신 릴리스를 받아 SHA-256을 확인하고 `/Applications`(쓸 수 없으면 `~/Applications`)에 넣는다.
그 뒤의 새 버전은 앱이 상단 바에서 알려 주고, 누르면 서명을 확인한 뒤 스스로 바꾼다.

Apple 공증을 받지 않은 자체 서명 앱이다. 설치 스크립트로 받으면 그대로 열리지만, 릴리스 페이지에서
브라우저로 zip을 받았다면 처음 열 때 시스템 설정 › 개인정보 보호 및 보안에서 "그래도 열기"를 눌러야 한다.
지울 때는 앱을 휴지통에 넣고, 설정과 창 배치는 `~/.config/bareframe`에 있다.

## 소스에서 빌드

```sh
./script/bootstrap   # Zig 0.16.0과 Cargo 의존성을 준비하고 빌드
./script/run         # 바로 실행
./script/check       # fmt, Clippy, 테스트, 빌드
```

필요한 도구와 의존성 고정은 [빌드 안내](docs/build.md)에 있다.

## 문서

- [사용 안내](docs/usage.md): 실행, 화면 구성, Settings, 단축키, 업데이트.
- [빌드 안내](docs/build.md): 필요한 도구, 의존성 고정, 검사 명령.
- [구조](docs/architecture.md): 모듈 배치와 설계 원칙.
- [Customizing](docs/customizing.md): 설정·동작·색·언어를 더하는 곳, 포크 배포.

## 라이선스

bareframe는 [GNU Affero General Public License v3.0 이상](LICENSE)(`AGPL-3.0-or-later`)으로 배포한다.
상업적 사용을 포함해 누구나 쓰고 고칠 수 있지만, 수정본을 배포하거나 네트워크 서비스로 제공하면 전체 소스를 같은 라이선스로 공개해야 한다.

함께 배포하는 구성 요소는 각자의 라이선스를 따른다.
- `vendor/gpui-libghostty`: MIT([LICENSE](vendor/gpui-libghostty/LICENSE)). 그 안의 Ghostty: MIT.
- Ghostty zsh 셸 통합(`Contents/Resources/shell-integration`): GPL-3.0([resources/shell-integration](resources/shell-integration)).
- GPUI: Apache-2.0.
