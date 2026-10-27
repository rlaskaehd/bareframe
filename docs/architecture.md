# 구조

bareframe은 GPUI(Zed의 UI 프레임워크)로 그린 창 안에 libghostty 터미널 surface를 native view로
얹고, 그 옆에 탐색기·편집기·Markdown 뷰어·Code review를 Pane으로 둔 macOS 앱이다.

```text
bareframe (src/)                    GPUI 뷰와 macOS 통합
├── app.rs, actions.rs, menu_bar.rs 시작, 동작·키 바인딩, 메뉴바
├── workspace.rs, workspace/        창 하나: 사이드바, 분할, 도구 패널, 팔레트, 저장·복원
├── terminal.rs                     Ghostty surface와 GPUI 입력의 연결
├── browser/, bin/bareframe-helper  Chromium(CEF) Pane: 메시지 펌프, 키보드 넘기기, 내부 DevTools
├── editor/, editor_tabs.rs         코드 편집기(찾기, 언어 서버, 상태 줄)
├── markdown/, diagram.rs           Markdown 뷰어와 Mermaid
├── explorer/, search.rs, review.rs 탐색기, 전역 검색, Code review
├── lsp/                            언어 서버 찾기·실행·대화
├── update/                         GitHub Releases 자동 업데이트
└── theme.rs, widgets.rs, syntax.rs 색, 작은 위젯, 문법 색
crates/bareframe-core/              GPUI 없는 모델(단독으로 테스트)
├── workspace.rs, layout.rs         탭·그룹, 분할 트리
├── settings.rs, keymap.rs          설정 정의와 파서, 단축키
├── state.rs                        창 배치 저장 형식
├── text.rs, ime.rs                 텍스트 버퍼, IME 조합
└── git.rs, lsp.rs, fuzzy.rs        git 출력 해석, LSP 프레이밍, 퍼지 검색
vendor/gpui-libghostty/             고정한 어댑터 사본(수정 내역은 BAREFRAME.md)
resources/shell-integration/        zsh 통합(Ghostty 것 + bareframe의 프롬프트 위치)
```

## 원칙

- **core는 GPUI에 의존하지 않는다.** 상태를 바꾸는 규칙(탭 이동, 분할, 설정 해석, git 출력)은
  core에 두고 `./script/check-core`로 UI 없이 테스트한다. 앱 쪽은 그 결과를 그린다.
- **설정은 한 곳에서 정의한다.** `settings.rs`의 `DEFS` 하나가 Settings 화면, 설정 파일 검증,
  Ghostty로 넘길 키를 모두 정한다.
- **동작 이름은 Ghostty와 같다.** `keymap.rs`의 `ACTIONS`가 단축키 기본값과 팔레트 목록의 근원이고,
  설정 파일의 `keybind` 문법도 Ghostty를 따른다.
- **터미널 세션은 Ghostty가 소유한다.** PTY와 셸 수명은 libghostty surface가 맡고, bareframe은
  native view의 배치·포커스·입력만 다룬다. 셸 이벤트(작업 디렉터리, 명령 시작·끝)는 zsh 통합과
  OSC 7/133으로 받는다.
- **브라우저는 엔진이 소유한다.** `browser/engine.rs`가 모든 Chromium 브라우저를 갖고 Pane은 id만
  가진다. 그래서 끝낼 때 브라우저를 모두 닫은 뒤에야 Chromium을 내리고, 그 뒤에 남는 참조가 없다.
  Chromium 콜백은 채널로 보내거나 값을 바꾸기만 하고, 엔진의 borrow를 Chromium 호출 너머로 잡지 않는다.
- **외부 프로그램은 사용자가 켜야 실행한다.** 언어 서버와 프로젝트 안의 Python은 사용자가 고르기
  전에는 실행하지 않는다.

## 의존성 고정

| 의존성 | 기준 |
| --- | --- |
| GPUI / gpui_platform | Zed `cc053a4a6fa2fd0e8793201ed9099466af1be0b1` |
| gpui-libghostty | `vendor/`의 upstream `2bbde4ae53a8e163cdbe05294bd09934b30c1af3` 기반 사본 |
| Ghostty | 어댑터에 포함된 `9f0e1719dc918368367d368bfe300f59bb68b5a4` 기반 소스 |

GPUI revision을 올리면 어댑터와 함께 맞춰야 한다. 어댑터를 고친 내용은
[vendor/gpui-libghostty/BAREFRAME.md](../vendor/gpui-libghostty/BAREFRAME.md)에 남긴다.
