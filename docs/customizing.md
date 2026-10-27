# 고치고 넓히기

bareframe을 직접 고치거나 포크해 쓰려는 사람을 위한 안내다. 무엇이 어디에 있는지는
[구조](architecture.md), 빌드 준비는 [빌드 안내](build.md)를 먼저 본다.

## 코드를 고치기 전에

- 설정 파일만으로 되는 것은 코드를 고치지 않아도 된다. 테마, 폰트, 단축키(`keybind`), 셸,
  프롬프트 위치 등은 `~/.config/bareframe/config`에서 바꾼다([사용 안내](usage.md#settings)).
- 매일 쓰는 번들은 `./script/bundle`(디버그, `target/bareframe.app`)로 만든다.
  `./script/run`은 번들 없이 바로 실행한다.
- 고친 뒤에는 `./script/check`를 통과시킨다. fmt, Clippy(`-D warnings`), core와 앱 테스트,
  빌드를 차례로 돈다. 상태를 바꾸는 규칙은 `crates/bareframe-core`에 두고 거기서 테스트한다.

## 자주 고치는 곳

### 설정 항목 더하기

1. `crates/bareframe-core/src/settings.rs`의 `DEFS`에 `def(...)` 하나를 더한다. 섹션과 그룹,
   키, 이름, 설명, 종류(`Toggle`·`Choice`·`Number`·`Integer`·`Text`), 기본값, 터미널로 넘기는 방식을
   정한다.
   - `App`: bareframe이 직접 읽는다.
   - `Same`: Ghostty 설정으로 같은 키 그대로 넘긴다.
   - `Map`: 다른 키로 바꿔서 넘긴다.
2. Settings 화면과 설정 파일 검증은 이것만으로 따라온다. `Same`/`Map`이면 터미널에도 바로 적용된다.
3. `App` 항목은 앱 쪽에서 `settings.flag(key)`, `settings.number(key)`, `settings.get(key)`로 읽는다.
   바뀔 때마다 적용해야 하면 `src/workspace/prefs.rs`의 `apply_settings`에서 처리한다.

### 동작과 단축키 더하기

1. `src/actions.rs`의 `actions!` 목록에 동작을 더한다.
2. 단축키를 바꿀 수 있게 하려면 core의 `keymap.rs` `ACTIONS`에 이름·설명·기본 키·범위를 적고,
   `actions.rs`의 `keymap_action`에서 그 이름을 동작으로 잇는다. 커맨드 팔레트와
   Keyboard shortcuts 화면은 `ACTIONS`를 따라 자동으로 보인다.
3. 창에서 처리할 동작이면 `src/workspace/commands.rs`의 `with_actions`에 처리기를 단다.
4. 메뉴에도 두려면 `src/menu_bar.rs`에 `MenuItem::action`을 더한다.

### 색 바꾸기

- 창과 터미널의 색은 `src/theme.rs`의 `DARK`와 `LIGHT` 두 벌이다. 강조색, 탭 그룹 색 6개,
  터미널 16색 팔레트가 여기 있다.
- 편집기 문법 색은 `src/syntax.rs`의 `color()`가 팔레트 칸을 고른다.
- Mermaid 다이어그램 색은 `src/diagram.rs`의 `THEME_VARIABLES`와 `SERIES`에 있다.

### 편집기 언어 더하기

1. `Cargo.toml`의 `arborium` features에 `lang-<이름>`을 더한다. 지금은 MIT 라이선스 문법만 켜 두었다.
2. 파일 이름으로 언어를 정하는 규칙은 `src/syntax.rs`의 `language_for`가 맡는다. 확장자는 arborium이
   알아서 정하고, 특별한 이름(`Dockerfile`, `.zshrc` 등)만 여기서 정한다.
3. 탐색기 아이콘이 필요하면 SVG를 `assets/icons/lang/`에 넣는다. 그다음 `src/assets.rs`의 `ICONS`와
   `src/file_icons.rs`의 `BY_EXTENSION`에 더한다. 아이콘 출처와 라이선스는 `assets/icons/README.md`에
   적는다.

### 언어 서버 더하기

core의 `lsp.rs` `SERVERS`에 실행 파일, 인자, 설치 명령, 확장자와 LSP 언어 id를 적는다.
서버는 사용자의 로그인 셸 PATH에서 찾고, 사용자가 상태 줄에서 켜기 전에는 실행하지 않는다.

### 셸 통합

`resources/shell-integration/bareframe/.zshenv`가 프롬프트를 맨 아래에 두는 일과 화면 지우기 감지를
맡는다. Ghostty의 zsh 통합(`zsh/`)은 고치지 않고 그대로 싣는다. bash·fish는 아직 통합이 없어
일반 터미널로만 동작한다.

## 포크를 따로 배포할 때

원래 bareframe과 섞이지 않게 세 가지를 바꾼다.

| 무엇 | 어디 |
| --- | --- |
| 업데이트를 받을 저장소 | `src/update/release.rs`의 `REPOSITORY` |
| 업데이트 서명 공개 키 | 같은 파일의 `PUBLIC_KEY`(`bareframe-release keygen`이 출력) |
| 번들 id | `src/update/install.rs`의 `BUNDLE_ID`와 `script/bundle`의 `CFBundleIdentifier` |

키를 만들고 릴리스하는 순서는 [README의 배포용 번들](../README.md#배포용-번들)을 따른다.
개인 키는 `~/.config/bareframe-release`에만 두고 저장소에 넣지 않는다.

## 라이선스

bareframe은 AGPL-3.0-or-later다. 수정본을 배포하거나 네트워크 서비스로 제공하면 전체 소스를 같은
라이선스로 공개해야 한다. 의존성을 더하면 `./script/bundle --release`가 그 라이선스 문서를
`THIRD_PARTY_NOTICES.txt`에 자동으로 모은다. 다만 AGPL과 함께 쓸 수 있는 라이선스인지는 직접
확인한다.
