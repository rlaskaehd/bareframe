# MVP 3 구현 계획 — 도구 패널, 설정, 편집 도구

기준: `mvp-2` 태그(2026-10-26). 상태: 2026-10-27 승인, 3a-1부터 구현. MVP 3을 두 단계로 나눈다.

- **MVP 3a**: MVP 2에서 넘어온 항목, 레이아웃 저장·복원, 커맨드 팔레트, Tools panel(Project explorer·Global search), 톱니 아이콘의 Settings·Keyboard shortcuts.
- **MVP 3b**: Markdown viewer·편집, Diff 버튼과 Code review 창, Code editor.

## 근거와 원칙

- 2026-10-26 설치된 Warp(Stable)를 직접 조작해 화면과 동작을 확인했다. 설정의 Appearance·Features·Keyboard shortcuts·Privacy·Editor and Code Review, Tools panel, Project explorer, Global search, Markdown 보기(Rendered/Raw), 코드 편집기, Code review 패널을 열어 봤다.
- 화면에 없는 세부 동작은 Warp 소스(`reference/warp/app/src`)와 Ghostty 소스(`reference/ghostty`)에서 확인했다. GPUI 구현 방식은 Zed 소스(`reference/zed/crates`)를 참고했다.
- **코드는 가져오지 않는다.** Warp 클라이언트는 AGPL-3.0, Zed의 editor·project·markdown·git 크레이트는 GPL-3.0이다. 동작과 UI만 참고하고, 구현은 허용 라이선스(MIT/Apache) 크레이트 위에 새로 쓴다. Zed 크레이트는 결합도가 높아(project·workspace·settings 의존) 라이선스와 별개로 의존성으로 쓸 수 없다.
- Warp의 AI·Agent·Warp Drive·계정·팀·결제·텔레메트리 기능과 명령 Block·입력 편집기에 묶인 기능은 제외한다.

## 공통 기반: Pane 종류 일반화 (3a 첫 단계)

지금 Pane은 모두 터미널이다(`panes: HashMap<PaneId, (Entity<TerminalPane>, _)>`). Warp는 Settings를 탭, Markdown·편집기·Diff를 분할 Pane으로 연다. 그래서 3a에서 Pane 종류를 먼저 일반화한다.

| 항목 | 내용 |
| --- | --- |
| 모델 | `PaneContent` = `Terminal` / `Settings` (3a) / `Markdown` / `Editor` / `Diff` (3b). 레이아웃 모델(`bareframe-core`)은 그대로 `PaneId`만 다룬다 |
| 렌더 | `render_layout`이 종류별 뷰를 그린다. 헤더(제목·`⋮`·`×`)는 모든 종류에 공통 |
| 표시·포커스 | `set_visible`·`set_frozen`은 터미널에만 적용한다. GPUI Pane은 팝업에 가려지지 않으므로 스냅샷이 필요 없다. 단, 옆의 터미널을 덮는 팝업은 지금처럼 그 터미널을 고정한다 |
| 포커스 | Ghostty NSView가 first responder를 가져가므로, GPUI Pane으로 포커스를 옮길 때 터미널이 확실히 포커스를 놓는지 확인한다(IME 포함) |
| 사이드바 | 카드의 아이콘·이름·위치를 종류별로 표시(예: 설정은 톱니 아이콘과 `Settings`, 파일은 파일 아이콘·파일 이름·디렉터리) |

## MVP 3a

### A0. MVP 2에서 넘어온 항목

| 항목 | 계획 |
| --- | --- |
| 위험한 붙여넣기·OSC 52 읽기 확인 | 완료(3a-6). Ghostty가 확인을 요청하면 셸 확인 창(내용 미리보기, `Cancel`/`Paste`·`Allow`)을 띄우고 터미널을 얼린다. `Esc`는 취소, 키로 승인하는 길은 두지 않는다. OSC 52는 `Clipboard access (OSC 52)`가 `Ask`일 때 같은 창을 쓴다. 창이 닫히기 전 Pane이 닫히면 거부한다 |
| 테마·폰트 저장 | Settings에서 바꾼 값은 설정 파일에 저장한다(A5). 메뉴·단축키로 바꾼 값도 같은 경로로 저장한다 |
| OSC 7 경로 신뢰 | 재검토 결과 지금 방식을 유지한다(3a-6). OSC 7 경로는 새 탭 시작 디렉터리, Explorer·Search 루트, 상태 저장에만 쓰고, 쓰는 곳마다 실제로 있는 디렉터리인지 확인한다. 터미널 안 프로그램이 다른 경로를 보내도 할 수 있는 일은 이미 읽을 수 있는 디렉터리를 보이게 하는 것뿐이고 명령을 실행하지 않는다. 프롬프트 직후 OSC 7만 받는 방식은 셸 통합이 없는 셸에서 cd 추적이 끊기므로 넣지 않는다 |

### A0-1. 레이아웃 저장·복원

| 항목 | 계획 |
| --- | --- |
| 저장 대상 | 창 크기·위치, 탭 순서, 탭 그룹(이름·색·접힘·고정), 탭 고정·색·이름, 분할 트리와 비율, 활성 탭·Pane, 최대화, 각 터미널 Pane의 작업 디렉터리, 사이드바·Tools panel 열림과 너비. 3b 이후에는 Markdown·편집기 Pane의 파일 경로 |
| 저장 시점 | 변경이 생기면 1초 묶어서, 그리고 종료할 때. `~/.config/bareframe/state.json`(설정 파일과 분리) |
| 복원 | 설정 Features의 `Restore windows, tabs, and panes on startup`(기본 켬). 터미널은 저장한 디렉터리에서 새 셸로 시작하고, 끝난 프로세스는 다시 실행하지 않는다. 디렉터리가 없으면 홈에서 시작 |
| 모델 | `bareframe-core`의 `Workspace`를 직렬화한다(`serde`). Pane id는 복원 시 다시 매긴다. 깨진 파일은 무시하고 경고만 남긴다 |

### A0-2. 커맨드 팔레트

| 항목 | 계획 |
| --- | --- |
| 열기 | `Cmd+Shift+P`(Ghostty `toggle_command_palette`와 같음). 상단 바 가운데 검색 자리에 맞춘 팝업 |
| 내용 | 앱의 모든 동작(Keyboard shortcuts 목록과 같은 출처)과 단축키 표시, 열린 탭·세션으로 이동, Settings 섹션 열기. 이름 퍼지 검색 |
| 실행 | Enter로 실행하고 닫는다. 팝업이 터미널 위에 겹치므로 지금 메뉴처럼 터미널을 정지 화면으로 바꾼다 |
| 구현(3a-7) | 퍼지 검색은 `bareframe-core::fuzzy`에 직접 구현했다(새 의존성 없음). 탭 이동은 `Go to tab: <이름>`, Settings는 `Settings: <섹션>` 항목이다 |

### A1. Tools panel

Warp 화면: 상단 바의 사이드바 토글 오른쪽에 렌치 아이콘(툴팁 `Tools panel ⌘\`). 누르면 탭 사이드바와 터미널 사이에 패널이 열린다. 패널 머리에 도구 아이콘 줄(Project explorer `⌃1`, Global search `⌃3`/`⇧⌘F`)과 닫기 `×`가 있다.

| 항목 | 계획 |
| --- | --- |
| 열기·닫기 | 상단 바 렌치 버튼과 `Cmd+\`. 같은 도구 아이콘을 다시 누르면 닫는다. 닫으면 포커스가 터미널로 돌아간다 |
| 배치 | 탭 사이드바 → Tools panel → 작업 영역 순. 너비는 끌어서 조절(사이드바와 같은 최소·최대 규칙) |
| 열림 상태 | Warp처럼 "Tools panel visibility is consistent across tabs" 설정을 둔다. 기본은 모든 탭 공통 |
| 도구 | Project explorer, Global search. 도구가 하나만 켜져 있으면 아이콘 줄을 숨긴다 |

### A2. Project explorer

| 항목 | 계획 |
| --- | --- |
| 루트 | 활성 탭 Pane들의 작업 디렉터리(OSC 7). 중첩되면 공통 상위로 합친다. cd하면 루트가 따라간다(확인: `cd ~/Sandbox/bareframe` 후 루트가 `bareframe`로 바뀜) |
| 로딩 | 한 단계씩 읽고 펼칠 때 더 읽는다(`ignore::WalkBuilder`, `max_depth(1)`). 디렉터리 먼저, 이름순 |
| 표시 | gitignore 대상은 숨기지 않고 기울임·흐리게 표시(Warp와 같음). 숨김 파일은 설정 `Show hidden files`(기본 켬). 확장자별 아이콘(Rust·Markdown·일반 파일·폴더) |
| 감시 | 패널이 보일 때만 `notify`(+ debouncer)로 감시하고 500ms로 묶어 갱신 |
| 클릭 | 한 번 클릭: 폴더는 펼침/접기, 파일은 열기. 3a에서는 시스템 기본 앱으로 열고, 3b에서 내장 편집기·Markdown viewer로 바꾼다 |
| 키보드 | 위/아래 이동, 오른쪽/왼쪽 펼침·접기, Enter 열기 |
| 우클릭 메뉴 | 파일: Open, Copy path, Copy relative path, Reveal in Finder. 폴더: `cd` to directory(활성 터미널에 `cd` 입력), Open in new tab, New file, Copy path, Reveal in Finder. 이름 바꾸기·삭제(휴지통)는 마지막에 넣는다 |
| 끌어 놓기 | 파일을 터미널에 놓으면 경로를 붙여 넣는다(셸 이스케이프) |

### A3. Global search

| 항목 | 계획 |
| --- | --- |
| 입력 | `Search in files` 입력칸, 대소문자(`Aa`)·정규식(`.*`) 토글. 300ms 입력 지연 후 검색, Enter는 즉시 검색, 빈 입력은 취소 |
| 범위 | Project explorer와 같은 루트. gitignore를 따른다(확인: `target/` 결과 없음) |
| 엔진 | `grep`(ripgrep 라이브러리) + `ignore` 병렬 탐색을 백그라운드 스레드에서 실행. 새 검색이 이전 검색을 취소. 이진 파일·64KB 넘는 줄은 건너뜀. 2만 건에서 중단 |
| 결과 | `N results in M files`. 루트 → 파일(아이콘, 이름, 상대 디렉터리, 건수 배지) → 일치한 줄(일치 부분 강조). 그룹은 접을 수 있다. 결과는 묶음 단위로 흘려 보낸다 |
| 클릭 | 3a: 시스템 기본 앱으로 연다. 3b: 내장 편집기에서 해당 줄·열로 연다 |
| 단축키 | `Cmd+Shift+F`: 패널을 열고 검색 칸에 포커스(선택된 텍스트가 있으면 채움) |

### A4. 톱니 아이콘과 Settings

Warp 화면: 상단 바 오른쪽 끝 톱니 아이콘(`Cmd+,`). Settings는 별도 창이 아니라 사이드바에 `Settings` 탭으로 열리는 Pane이다. 왼쪽에 검색과 섹션 목록, 아래에 `Open settings file`. 이미 열려 있으면 그 탭으로 이동한다.

섹션: **Appearance, Features, Keyboard shortcuts, About**. Account·Agents·Code(Indexing)·Cloud platform·Teams·Warpify·Referrals·Warp Drive·Privacy(텔레메트리·충돌 보고)는 넣지 않는다.

| 섹션 | 넣는 항목 | Ghostty·앱 연결 |
| --- | --- | --- |
| Appearance — Themes | 현재 테마(Dark/Light 카드), Sync with OS(라이트·다크 테마 각각 선택) | 앱 테마 |
| Appearance — Window | 새 창 크기 지정(열·행), 창 불투명도, 배경 블러, UI 확대 | `background-opacity`, `background-blur` |
| Appearance — Tools panel | Project explorer / Global search 표시 | A1 |
| Appearance — Panes | 비활성 Pane 흐리게, 마우스를 따라 포커스 | `unfocused-split-opacity`, `focus-follows-mouse` |
| Appearance — Text | 터미널 폰트(가족·굵기·크기·줄 높이·기본값으로), 모든 시스템 폰트 보기, 얇은 획, 최소 대비, 합자 | `font-family`, `font-style`, `font-size`, `adjust-cell-height`, `font-thicken`, `minimum-contrast`, `font-feature` |
| Appearance — Cursor | 커서 모양(Bar/Block/Underline), 깜박임 | `cursor-style`, `cursor-style-blink` |
| Appearance — Tabs | 탭 표시기, 탭 닫기 버튼 위치, 새 탭에 활성 탭 색 유지, 복원 창에서 사이드바 표시, 디렉터리별 탭 색 | 앱 |
| Appearance — Full-screen apps | alt-screen 여백 | `window-padding-*` |
| Features — General | 시작 시 창·탭·Pane 복원(결정 1), 종료 전 경고, 마지막 창을 닫으면 종료, 로그인 시 시작, 마우스 휠 줄 수 | `confirm-close-surface`, `quit-after-last-window-closed`, `mouse-scroll-multiplier` |
| Features — Session | 새 세션의 셸, 새 세션의 작업 디렉터리(Home/이전 세션/지정), 닫은 세션 다시 열기(유예 시간) | 앱(`LaunchConfig`) |
| Features — Keys | 왼쪽·오른쪽 Option을 Meta로, Ctrl+Tab 동작, 전역 단축키(나중) | `macos-option-as-alt` |
| Features — Terminal | 마우스·스크롤·포커스 리포팅, 경고음, 더블클릭 단어 문자, 선택 시 복사, 클립보드 접근(OSC 52), 새 탭 위치(현재 탭 뒤/맨 끝), 가운데 클릭 붙여넣기, 우클릭 동작 | `mouse-reporting`, `bell-features`, `selection-word-chars`, `copy-on-select`, `clipboard-read`·`clipboard-write`, … |
| Features — Notifications | 오래 걸린 명령이 끝나면 데스크톱 알림(N초 이상) | `COMMAND_FINISHED` 이벤트 |
| About | 버전, 라이선스 고지(셸 통합 GPLv3 포함) | 앱 |

- 각 항목은 같은 행 형식(이름, 설명, 오른쪽 컨트롤)을 쓴다. 컨트롤은 토글, 드롭다운, 숫자 입력, 슬라이더, 폰트 선택 5종이다. 항목은 `읽기·쓰기` 함수 쌍과 검색어로 정의한다(Zed settings_ui 방식).
- 검색은 모든 섹션의 항목 이름·설명에서 찾는다.
- 저장: 지금의 `~/.config/bareframe/config`(Ghostty식 `key = value`)를 그대로 쓴다. Settings에서 바꾸면 해당 줄만 고쳐 쓰고(주석·순서 유지), 파일이 바뀌면 다시 읽는다. `Open settings file`은 이 파일을 기본 편집기로 연다(3b부터 내장 편집기).
- 터미널에 적용되는 값은 지금처럼 Ghostty 설정을 다시 만들어 모든 surface에 적용한다. libghostty가 재적용을 지원하지 않는 항목은 "새 세션부터 적용"으로 표시한다.

**3a-4 구현 결과(2026-10-27)**: 위 표 중 아래 항목은 넣었다. 테마(Sync with OS 포함), 텍스트(폰트·크기·굵기·줄 높이·획 굵게·최소 대비·합자), 커서, 터미널 여백, 새 탭 색 유지, Tools panel 도구 표시, 시작 시 복원, 휠 배율, 새 세션 셸·작업 디렉터리·새 탭 위치, Option as Meta, 마우스 리포팅, 선택 시 복사, 단어 경계 문자, 클립보드 접근, About.
남은 항목과 이유:
- 창 불투명도·블러, UI 확대: GPUI 창 배경 투명화와 모든 색의 알파 처리, rem 기반 크기가 필요하다.
- 비활성 Pane 흐리게, 마우스를 따라 포커스: Ghostty의 해당 설정은 앱 계층(macOS·GTK)에서 처리해 임베드에서는 동작하지 않는다. 네이티브 뷰 투명도와 마우스 추적을 직접 구현해야 한다.
- 종료 전 경고, 마지막 창을 닫으면 종료(끄기), 로그인 시 시작, 데스크톱 알림, 경고음, 새 창 크기 지정, 탭 표시기·닫기 버튼 위치, 디렉터리별 탭 색: 각각 별도 기능(확인 창, 창 다시 열기, 로그인 항목, 알림 권한 등)이 필요해 후속 작업으로 둔다.

### A5. Keyboard shortcuts

Warp 화면: `Configure keyboard shortcuts` 제목, `Search by name or by keys` 검색, 동작 이름과 키 캡 목록. 충돌하는 키는 주황 테두리로 표시. 행을 누르면 `Press new keyboard shortcut` 줄과 `Clear`·`Default`·`Cancel`·`Save` 버튼이 펼쳐진다.

| 항목 | 계획 |
| --- | --- |
| 목록 | 앱이 정의한 모든 동작: 이름, 설명, 그룹, 기본 키, 사용자 키. 이름순 |
| 기본값 | Ghostty macOS 기본값(아래 표). 앱 고유 동작(사이드바·Tools panel·Settings 등)은 Warp 기본값. 예외: 탭 그룹 `Cmd+G`/`Cmd+Shift+G`와 사이드바 `Cmd+B`/`Cmd+Shift+B`는 지금 키를 유지한다 |
| 녹화 | 행을 누르면 `cx.intercept_keystrokes`로 키 하나를 녹화한다. 녹화 중에는 앱 단축키가 실행되지 않는다. `Clear`(해제), `Default`(기본값), `Cancel`, `Save` |
| 충돌 | 같은 키를 쓰는 동작이 둘 이상이면 키 캡을 주황색으로 표시하고 "This shortcut conflicts with other keybinds"를 보인다. 저장은 막지 않는다 |
| 검색 | 이름 부분 일치 + 키 조합(`cmd d`) 일치 |
| 저장 | 화면에서 바꾸면 바꾼 것만 설정 파일에 Ghostty 문법으로 쓴다: `keybind = cmd+t=new_tab`, 해제는 `keybind = cmd+g=unbind`. 같은 줄을 손으로 써도 되고, 어느 쪽으로 바꿔도 화면과 파일이 같은 값을 보인다 |
| 터미널 동작 | `clear_screen`·`select_all`·`scroll_*`·`jump_to_prompt` 같은 터미널 동작은 shim의 `ghostty_surface_binding_action`으로 Ghostty에 이름 그대로 넘긴다 |

Ghostty macOS 기본값 중 이번에 넣는 것(`src/config/Config.zig` `Keybinds.init`):

| 동작 | 키 | 지금 bareframe |
| --- | --- | --- |
| new_tab / close_surface / close_tab / close_window / quit | `cmd+t` / `cmd+w` / `cmd+alt+w` / `cmd+shift+w` / `cmd+q` | `cmd+alt+w` 없음 |
| new_split right / down | `cmd+d` / `cmd+shift+d` | 같음 |
| goto_split previous / next, 방향 | `cmd+[` / `cmd+]`, `cmd+alt+방향키` | `cmd+[`/`]` 없음 |
| resize_split, equalize_splits, toggle_split_zoom | `cmd+ctrl+방향키`, `cmd+ctrl+=`, `cmd+shift+enter` | 크기·균등 없음 |
| previous_tab / next_tab, goto_tab 1–8, last_tab | `cmd+shift+[`/`]`, `ctrl+shift+tab`/`ctrl+tab`, `cmd+1`–`8`, `cmd+9` | `ctrl+tab` 없음 |
| copy / paste / select_all / clear_screen | `cmd+c` / `cmd+v` / `cmd+a` / `cmd+k` | `cmd+a`·`cmd+k` 없음 |
| 폰트 크기 | `cmd+=`·`cmd++` / `cmd+-` / `cmd+0` | 같음 |
| 스크롤, 프롬프트 이동 | `cmd+home`/`end`/`pgup`/`pgdn`, `cmd+up`/`down` | 없음 |
| open_config / reload_config | `cmd+,` / `cmd+shift+,` | 없음(`cmd+,`는 Settings 열기로) |
| toggle_fullscreen | `cmd+enter`, `cmd+ctrl+f` | 없음 |
| start_search 등 스크롤백 검색 | `cmd+f`. 다음·이전은 Ghostty의 `cmd+g`/`cmd+shift+g` 대신 검색 칸의 `Enter`/`Shift+Enter`(탭 그룹 키 유지) | 없음. libghostty 검색 지원 여부를 먼저 확인 |

## MVP 3b

### B1. Markdown viewer와 편집

Warp 화면: `.md`를 열면 오른쪽 분할 Pane으로 열린다. 머리에 파일 이름, `Rendered`/`Raw` 전환, `⋮`, `×`. Rendered는 읽기 전용이고, 코드 블록에 복사·터미널로 보내기 버튼이 있다. Raw는 줄 번호가 있는 코드 편집기다.

| 항목 | 계획 |
| --- | --- |
| 여는 경로 | Project explorer·Global search에서 `.md` 클릭. 설정 `Open Markdown files in the Markdown viewer`(기본 켬)와 `Choose a layout to open files`(Split Pane/New Tab) |
| 렌더링 | `pulldown-cmark` → GPUI 요소. 제목, 문단, 굵게·기울임·취소선, 인라인 코드, 코드 블록(복사 버튼, 셸 블록은 "터미널에 붙여넣기"), 목록·체크 목록, 인용, 구분선, 링크(`open`), 로컬 이미지, GFM 표 |
| 편집 | `Cmd+E` 또는 `Raw`로 같은 파일을 B3 편집기로 전환(스크롤 위치 유지). 실시간 미리보기는 하지 않는다(Warp와 같음) |
| 갱신 | 파일이 바뀌면 다시 읽는다 |
| 제외 | Mermaid, 인라인 HTML |
| 구현(3b-1) | `src/markdown.rs`: `pulldown-cmark` 이벤트를 블록·인라인 모델로 바꿔 GPUI 요소로 그린다(파서 테스트 포함). 다른 Markdown으로 가는 링크는 같은 Pane에서 연다. 웹 이미지는 불러오지 않는다(파일을 연 사실이 외부 서버에 알려지지 않게). 설정은 Features › Files에 두었다. `Raw` 전환은 B3 편집기와 함께 3b-4에서 붙인다 |

### B2. Diff 버튼과 Code review 창

Warp 화면: 상단 바 오른쪽에 `± +N −M` 버튼(초록·빨강). 누르면 오른쪽 패널(`⇧⌘+`)이 열린다. 머리에 `경로: 브랜치`, 파일 수, `+N −M`. 비교 대상 선택(`Uncommitted changes` / 브랜치 목록). 파일별 카드에 이름·복사·`+N −M`·되돌리기·편집기에서 열기가 있다. 그 아래는 줄 번호가 있는 통합(unified) diff다.

| 항목 | 계획 |
| --- | --- |
| 버튼 | 활성 Pane 작업 디렉터리의 git 저장소 기준 `git diff --numstat HEAD`와 추적하지 않는 파일의 줄 수. 명령이 끝날 때(`COMMAND_FINISHED`), 디렉터리가 바뀔 때, 저장소 감시로 갱신. 변경이 없거나 저장소가 아니면 숨긴다. 설정으로 버튼·숫자 표시를 끌 수 있다 |
| 패널 | 오른쪽 패널(분할 Pane이 아닌 창 오른쪽 영역). 너비 조절 |
| 비교 대상 | 기본 `Uncommitted changes`(작업 트리 대 HEAD). 브랜치를 고르면 `git merge-base`와 비교 |
| 데이터 | `git` CLI: `status --porcelain=2 -z --untracked-files=all`, `diff --numstat`, `diff -z HEAD -- 파일`, `show HEAD:경로`. 큰 diff(4MB, 한 줄 5000자 초과)는 접어 둔다 |
| 표시 | 파일 카드 접기·펼치기, 통합 diff(추가 초록, 삭제 빨강, 양쪽 줄 번호). `uniform_list`로 보이는 줄만 그린다 |
| 동작 | 경로 복사, 파일 변경 되돌리기(확인 창 후 `git restore`·추적하지 않은 파일은 휴지통), 편집기에서 열기(B3) |
| Commit | 패널 머리의 `Commit` 버튼: 메시지 입력 창, 변경 전체 또는 고른 파일을 `git add`·`git commit`. 저장소 상태(충돌·병합 중)면 막고 이유를 표시 |
| 헝크 되돌리기 | diff의 헝크마다 되돌리기 버튼. 확인 후 해당 헝크만 작업 트리에서 되돌린다(`git apply -R`에 그 헝크 패치를 넘김). 편집기에 열린 파일이면 버퍼도 다시 읽는다 |
| 제외 | Push·PR, 리뷰 코멘트·AI 컨텍스트, 헝크 단위 스테이징, 나란히 보기 |
| 구현(3b-2) | `src/repo.rs`(git 실행, 테스트는 임시 저장소), `src/review.rs`(패널, 가변 높이 `list`), core `git.rs`(출력 파서). 단축키는 Warp의 `⇧⌘+`가 Ghostty 글자 크기와 겹쳐 `Cmd+Shift+\`. 새 파일 되돌리기는 `trash` 크레이트(NSFileManager)로 휴지통. 읽기 전용 갱신에서는 `core.fsmonitor`·외부 diff·textconv를 꺼 저장소 설정이 프로그램을 실행하지 못하게 했다. 브랜치 비교 중에는 되돌리기를 숨긴다 |

### B3. Code editor

Warp 화면: 파일을 열면 오른쪽 분할 Pane의 편집기에 열리고, 여러 파일은 한 편집기 Pane 안의 파일 탭으로 묶인다(설정 `Group files into single editor pane`). 줄 번호, 현재 줄 강조, 구문 강조, 아래쪽 상태 줄이 있다.

| 항목 | 계획 |
| --- | --- |
| 버퍼 | `ropey`. 줄 단위 편집, 실행 취소/다시 실행 |
| 입력 | `rename.rs`의 GPUI 입력 처리(`EntityInputHandler`)를 여러 줄로 넓힌다: 커서 하나와 선택, 클립보드, 단어·줄 이동, 한글 IME 조합(후보 창 위치 포함) |
| 렌더링 | `uniform_list`로 보이는 줄만 그리고, 줄 모양(`ShapedLine`)을 줄·버전별로 캐시. 줄바꿈 없이 가로 스크롤(자동 줄바꿈은 나중) |
| 구문 강조 | `tree-sitter` + `arborium` 묶음 문법(Rust, Markdown, TOML, JSON, YAML, Shell, Python, JS/TS 등). 백그라운드에서 증분 파싱, 2MB 넘으면 끔 |
| 파일 | `Cmd+S` 저장, 탭과 Pane 헤더에 변경 표시, 저장하지 않고 닫으면 확인, 디스크에서 바뀌면 다시 읽기(변경 중이면 확인). 이진 파일은 열지 않고, 큰 파일은 읽기 전용 |
| 묶음 | 한 탭의 파일들은 편집기 Pane 하나의 파일 탭으로 묶는다(설정으로 끌 수 있음) |
| 기타 | 줄 번호 설정(절대/상대/끔), 찾기·바꾸기(`Cmd+F`), 줄로 이동, 자동 저장(기본 끔) |
| 제외 | LSP(정의 이동·포맷), 다중 커서, Vim 모드, 미니맵 |

## 구현 순서와 완료 기준

| 단계 | 내용 | 완료 기준 |
| --- | --- | --- |
| 3a-1 | Pane 종류 일반화, 상단 바 버튼 자리(렌치·톱니) | 터미널 동작 회귀 없음, 빈 GPUI Pane을 분할로 열고 닫기 |
| 3a-1b | 레이아웃 저장·복원 | 탭·그룹·분할·작업 디렉터리를 재시작 후 그대로 복원, 깨진 상태 파일에서도 시작 |
| 3a-2 | Tools panel + Project explorer | cd에 따라 루트 변경, 큰 저장소(이 저장소의 `reference/`)에서 펼치기 지연 없음, gitignore 표시, 파일 변경 반영 |
| 3a-3 | Global search | `reference/` 전체 검색 중 입력해도 UI가 멈추지 않음, 취소·재검색, 결과 클릭으로 열기 |
| 3a-4 | Settings 화면과 저장 | 모든 항목이 파일에 저장되고 재시작 후 유지, 파일을 직접 고쳐도 반영, 검색 |
| 3a-5 | Keyboard shortcuts | 녹화·해제·기본값·충돌 표시, 저장 후 재시작해도 유지, Ghostty 기본값표와 일치 |
| 3a-6 | 붙여넣기 확인 창, OSC 7 재검토 | 위험한 붙여넣기가 확인 후에만 들어감 |
| 3a-7 | 커맨드 팔레트 | 모든 동작을 이름으로 찾아 실행, 탭 이동, 단축키 표시가 Keyboard shortcuts와 일치 |
| 3b-1 | Markdown viewer | 이 저장소 `docs/*.md`가 Warp와 같은 요소로 보임, 링크·이미지·코드 복사 |
| 3b-2 | Diff 버튼과 Code review 창 | `+N −M`이 `git diff --numstat`과 일치, 파일별 diff, 파일·헝크 되돌리기, Commit 후 버튼 숫자 갱신 |
| 3b-3 | Code editor | 한글 IME, 1만 줄 파일 스크롤이 매끄러움, 저장·변경 표시·닫기 확인, 구문 강조 |
| 3b-4 | 연결 | Markdown `Raw` 전환, Explorer·Search·Diff에서 편집기로 열기(줄·열) |

각 단계는 지금처럼 `./script/check`, 검증 앱에서의 화면 확인, 문서(`usage.md`, 검증 기록) 갱신 후 로컬 커밋한다.

## 새 의존성(허용 라이선스)

| 단계 | 크레이트 | 라이선스 |
| --- | --- | --- |
| 3a | `ignore` 0.4, `grep` 0.4, `notify` 8 + `notify-debouncer-full`, `regex` 1, `trash`(삭제 시), `serde`·`serde_json`(상태 저장), 퍼지 검색은 직접 구현하거나 `nucleo-matcher` | MIT/Unlicense, CC0/Artistic(notify), MIT, MIT/Apache, MPL-2.0(nucleo) |
| 3b | `pulldown-cmark` 0.13, `ropey` 1, `tree-sitter` 0.25+, `arborium` 2, `similar` 2 또는 `imara-diff` 0.2 | MIT, MIT, MIT, MIT/Apache, Apache |

설치 전 각 크레이트의 라이선스 원문을 다시 확인하고 `deny`식 목록으로 기록한다.

## 위험

- **여러 줄 IME 편집기**가 가장 큰 작업이다. 줄을 넘는 UTF-16 범위, 후보 창 위치, 조합 중 커서 이동을 직접 구현해야 한다. 3b에서 편집기를 별도 단계로 두고 한글 입력 시나리오를 먼저 검증한다.
- **네이티브 터미널과 GPUI Pane 사이의 포커스**: 키와 IME가 잘못된 쪽으로 가지 않는지 분할 상태에서 확인한다.
- **큰 저장소**: 탐색·검색·diff를 모두 백그라운드에서 하고 결과를 묶어서 UI로 보낸다.
- **설정 재적용 범위**: libghostty가 실행 중 바꿀 수 없는 설정은 새 세션부터 적용한다고 표시한다.

## 결정 사항 (2026-10-27)

1. 레이아웃 저장·복원과 커맨드 팔레트는 3a에 넣는다(A0-1, A0-2).
2. 설정은 `~/.config/bareframe/config`(Ghostty식, `keybind` 포함) 하나로 두고, 단축키는 Keyboard shortcuts 화면에서 수정할 수 있어야 한다.
3. 탭 그룹 `Cmd+G`/`Cmd+Shift+G`는 지금 키를 유지한다. 사이드바 열기·닫기에 `Cmd+B`를 추가했다(`Cmd+Shift+B`도 유지).
4. 라이선스는 `AGPL-3.0-or-later`로 정했다(상업적 사용 허용, 배포·서비스 시 전체 소스 공개). `LICENSE`와 README에 기록했다. About 화면에 같은 고지를 넣는다.
5. Code review 창에 Commit 버튼과 헝크 되돌리기를 넣는다(B2).
