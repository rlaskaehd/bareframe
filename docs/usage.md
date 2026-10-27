# 사용 안내

빌드 준비가 끝나면 다음 명령으로 실행한다.

```sh
./script/run --clean-shell
./script/run --cwd ~/my-project
```

일반 실행은 `/bin/zsh -l -i`, clean shell은 `/bin/zsh -f -i`를 사용한다.
일반 실행의 zsh는 Ghostty의 zsh 셸 통합(번들의 `Resources/shell-integration/zsh`)을 `ZDOTDIR`로 불러온다.
통합 스크립트는 사용자 `ZDOTDIR`과 초기화 파일을 그대로 읽은 뒤, 탭 제목과 작업 디렉터리를 알려 주는 hook만 추가한다(GPLv3, `resources/shell-integration/README`).
clean shell은 사용자의 zsh 초기화 파일을 읽지 않아 프롬프트 확장과 플러그인의 영향을 줄인다.
시스템의 `/etc/zshenv`와 상속한 환경 변수는 계속 적용될 수 있다.
사용자 zsh·Ghostty 설정 파일을 수정하지 않는다.

## 옵션

| 옵션 | 동작 |
| --- | --- |
| `--cwd DIRECTORY` | 시작 작업 디렉터리. 기본값은 home(`~`). 상대 경로는 실행 시 현재 디렉터리 기준 |
| `--shell EXECUTABLE` | 절대 경로로 지정한 셸 실행 파일. 기본 `/bin/zsh` |
| `--clean-shell` | zsh의 사용자 초기화 파일을 생략 |
| `--help`, `--version` | CLI 안내와 버전 표시 |

MVP 1의 셸 실행 모드는 zsh를 기준으로 한다. 다른 셸을 지정한 경우에도 `-l -i` 플래그를 사용하므로 해당 셸의 호환성을 확인해야 한다.

## 탭과 분할

| 키 | 동작 |
| --- | --- |
| `Cmd+T` | 새 탭. 현재 Pane의 작업 디렉터리에서 시작한다(셸이 OSC 7로 알려 준 경우) |
| `Cmd+D` / `Cmd+Shift+D` | 오른쪽 / 아래로 분할 |
| `Cmd+Option+방향키` | 그 방향의 Pane으로 포커스 이동 |
| `Cmd+Shift+Enter` | 활성 Pane 최대화 / 되돌리기. 분할·닫기·다른 Pane으로 포커스 이동 시 풀린다 |
| `Cmd+Shift+[` / `Cmd+Shift+]` | 이전 / 다음 탭 |
| `Cmd+1`–`Cmd+8`, `Cmd+9` | 번호로 탭 선택, 마지막 탭 |
| `Cmd+W` | 현재 Pane 닫기. 탭의 마지막 Pane이면 탭을 닫는다. 마지막 탭이면 창을 닫지 않고 새 탭으로 바꾼다 |
| `Cmd+G` / `Cmd+Shift+G` | 현재 탭으로 새 그룹 만들기 / 그룹에서 빼기 |
| `Cmd+B`, `Cmd+Shift+B` | 탭 사이드바 열기·닫기(상단 바 왼쪽 버튼과 같음) |
| `Cmd+,` | Settings 탭 열기(상단 바 오른쪽 톱니 버튼과 같음). 이미 열려 있으면 그 탭으로 이동 |
| `Cmd+\` | Tools panel 열기·닫기(상단 바 렌치 버튼과 같음) |
| `Ctrl+1` / `Ctrl+3` | Project explorer / Global search 열기. 이미 그 도구가 보이면 Tools panel을 닫는다 |
| `Cmd+Shift+F` | Global search를 열고 검색 칸으로 이동 |
| `Cmd+Shift+W` | 창 닫기 |
| `Cmd+Shift+P` | 커맨드 팔레트 열기·닫기(View 메뉴 `Command Palette`와 같음) |
| `Cmd+Shift+\` | Code review 패널 열기·닫기(상단 바 `± +N −M` 버튼과 같음). Warp의 `⇧⌘+`는 Ghostty에서 글자 크기 키라서 Tools panel(`Cmd+\`)의 반대쪽 짝으로 정했다 |

- 탭은 왼쪽 사이드바에 Warp처럼 원형 `>_` 아이콘(탭 색상을 고르면 같은 색), 이름, 위치 순의 카드로 표시된다. Warp처럼 탭 사이에 구분선은 없고, 모든 카드는 한 줄짜리 Settings 카드와 같은 높이다. 분할된 탭은 활성 Pane 하나만 보여 준다.
- 사이드바 위 `+`로 새 탭을 연다. 탭에 마우스를 올리면 오른쪽 위에 `⋮`(우클릭 메뉴와 같음)와 `×`(그 탭의 모든 Pane 닫기)가 나타난다.
- 사이드바 위 `Search tabs...`에 입력하면 제목이나 위치에 그 글자가 있는 탭만 보인다(한글 입력 가능). `Enter`는 첫 결과로 이동하며 검색을 끝내고, `Esc`는 검색을 지운다.
- 사이드바 오른쪽 가장자리를 끌어 너비를 바꾼다(기본 248, 최소 200, 최대 창 너비의 절반). 창을 줄이면 사이드바도 창 너비의 절반까지 줄어든다.
- 탭 카드는 Warp처럼 윗줄에 이름, 아랫줄에 위치(`~/Sandbox/bareframe`)를 표시한다. 이름은 직접 지은 이름, 없으면 실행 중이거나 마지막으로 실행한 명령, 명령을 실행한 적 없으면 `New session`이다. 명령은 셸 통합이 알려 주므로 clean shell에서는 늘 `New session`이다. Pane 헤더와 창 제목은 셸이 알려 주는 제목(위치나 실행 중인 명령)을 그대로 쓴다.
- 새 탭과 분할은 활성 Pane의 현재 위치에서 시작한다.
- Pane 헤더를 끌어 다른 Pane 위에 놓으면 그 Pane의 가장 가까운 쪽(왼쪽·오른쪽·위·아래)으로 옮겨진다. 놓일 자리는 반투명한 파란 영역으로 미리 보인다. 사이드바의 탭 카드(여러 개 선택했으면 선택한 탭 전부)를 현재 탭의 Pane 위에 놓으면 그 탭의 Pane들이 같은 방식으로 현재 탭에 합쳐지고, 빈 탭은 사라진다. 반대로 Pane 헤더를 사이드바의 탭 카드 위(그 탭 앞)나 목록 아래 빈 곳(맨 끝)에 놓으면 그 Pane이 새 탭으로 떨어져 나온다. 그룹 헤더에 놓으면 그 그룹의 끝에 들어간다. 끄는 동안 터미널은 정지 화면으로 바뀐다.
- zsh(clean shell 제외)에서는 Warp처럼 프롬프트가 화면 맨 아래에서 시작하고 출력은 위로 올라간다. `clear`, `Ctrl+L`, `Cmd+K` 뒤에도 맨 아래로 돌아간다. 창이나 Pane이 커져도 맨 아래에 남도록, 시작할 때와 화면을 지운 뒤에는 빈 줄 200개를 스크롤백에 쌓아 둔다(맨 위까지 올리면 빈 영역으로 보인다).
- 창 크기·위치, 탭·그룹·고정·색·이름, 분할과 비율, 활성 탭·Pane, 사이드바 상태, 각 터미널의 작업 디렉터리를 `~/.config/bareframe/state`에 2초마다(바뀐 경우만)와 종료할 때 저장하고, 다음 실행 때 복원한다. 터미널은 저장한 위치에서 새 셸로 시작하며 실행 중이던 프로그램은 다시 실행하지 않는다. `--cwd`로 위치를 지정해 실행하면 복원하지 않는다. 읽을 수 없는 상태 파일은 경고만 남기고 새로 시작한다.
- 창 맨 위 상단 바를 끌면 창이 이동하고, 더블클릭하면 확대된다. 상단 바 가운데는 검색창 자리로 비워 두었다.

### 커맨드 팔레트

- `Cmd+Shift+P`로 창 위쪽 가운데에 연다. Keyboard shortcuts 목록의 모든 동작(이름순, 지금 쓰는 단축키 표시), `Settings: <섹션>`, 열린 탭마다 `Go to tab: <이름>`(위치 함께 표시)이 있다.
- 입력한 글자가 이름에 순서대로 들어 있으면 찾는다(`spr` → `Split pane right`). 단어 첫 글자와 이어진 글자가 맞을수록 위에 오고, 맞은 글자는 굵게 표시한다.
- `↑`/`↓`로 고르고 `Enter`나 클릭으로 실행한다. `Esc`, 바깥 클릭, `Cmd+Shift+P`로 닫는다. 터미널 동작(`Clear screen` 등)은 활성 Pane에서 실행된다.
- 팔레트가 열린 동안 터미널은 정지 화면으로 바뀌었다가 닫으면 돌아온다.

### Tools panel과 Project explorer

- 탭 사이드바와 Pane 사이에 열린다. 오른쪽 가장자리를 끌어 너비를 바꾼다. 열림 상태와 너비는 레이아웃과 함께 저장된다.
- Project explorer와 Global search의 루트는 활성 Pane 터미널의 작업 디렉터리다(활성 Pane이 파일이면 그 탭의 첫 터미널). 다른 Pane이나 탭으로 옮기거나 `cd`하면 루트가 따라간다.
- 폴더는 펼칠 때 한 단계씩 읽는다. gitignore 대상은 숨기지 않고 기울임체로 흐리게 표시한다(`.git` 포함). 패널이 열린 동안 파일 변경을 감시해 반영한다.
- 클릭: 폴더는 펼치기·접기, 파일은 기본 앱으로 연다(MVP 3b에서 내장 Markdown·편집기로 바뀐다). 실행 권한이 없는 문서 형식만 연다. 텍스트·코드는 기본 텍스트 편집기로(`.py` 등의 기본 앱이 실행기일 수 있어서), 이미지·PDF는 기본 뷰어로 열고, 심볼릭 링크는 가리키는 파일 기준으로 판단한다. 그 밖의 파일(스크립트, `.command`, `.app` 등)은 Finder에서 위치만 보여 준다. 키보드: 위/아래 선택, 오른쪽/왼쪽 펼치기·접기(왼쪽은 상위 폴더로), `Enter` 열기.
- 우클릭 메뉴: 폴더는 `cd to directory`(활성 터미널이 프롬프트에서 기다리는 중이면 입력 중이던 줄을 지우고 `cd` 실행. 프로그램(vim, less, ssh 등)이 실행 중이거나 셸 통합이 없어 알 수 없으면 그 폴더에서 새 탭을 연다. 경로에 제어 문자가 있으면 입력하지 않는다), `Open in new tab`. 파일은 `Open`. 공통으로 `Reveal in Finder`, `Copy path`, `Copy relative path`.

### Global search

- Tools panel 머리의 돋보기 탭. 검색 범위는 Project explorer와 같은 루트이고, gitignore 대상·숨김 파일·이진 파일·64KB 넘는 줄은 건너뛴다(ripgrep과 같음).
- 입력이 멈추고 300ms 뒤에 검색하고, `Enter`는 바로 검색한다. 새 검색은 이전 검색을 멈춘다. 결과는 찾는 대로 쌓이며 2만 건에서 멈춘다. `Esc`는 터미널로 돌아간다.
- `Aa`는 대소문자 구분, `.*`는 정규식. 정규식이 틀리면 결과 자리에 오류를 보인다.
- 결과는 루트 → 파일(이름, 상대 디렉터리, 건수) → 일치한 줄(일치 부분 강조)로 묶이고, 루트·파일 줄을 눌러 접는다. 줄을 누르면 파일을 연다. Markdown은 Markdown viewer로 열고(해당 줄로 이동하는 것은 3b-4), 그 밖의 파일은 Project explorer와 같은 규칙으로 연다.

### Markdown viewer

- Project explorer나 Global search에서 `.md`(`.markdown` 등 포함) 파일을 열면 bareframe 안에서 렌더링해 보여 준다. Settings › Features › Files의 `Open Markdown files in the Markdown viewer`(기본 켬)를 끄면 기본 앱으로 연다. `Choose a layout to open files`로 활성 Pane 오른쪽 분할(기본)과 새 탭 중에 고른다. 이미 열린 파일은 그 Pane으로 이동한다.
- 제목(Warp와 같은 크기 비율), 문단, 굵게·기울임·취소선, 인라인 코드, 코드 블록, 목록·체크 목록, 인용, 구분선, 표, 링크, 이미지를 보인다. 인라인 HTML과 HTML 블록은 보이지 않는다. ` ```mermaid ` 코드 블록은 다이어그램으로 그린다(merman, 앱 테마에 맞춰 어둡게·밝게). flowchart, sequence, state, ER, class, gantt, git graph, journey, mindmap, pie, timeline을 지원하고, 그리지 못하면 원문과 이유를 보인다.
- 코드 블록 오른쪽 위 `Copy`는 내용을 클립보드에 복사한다. 셸 코드 블록(`sh`, `bash`, `zsh`, `shell`, `console`, `fish`)에는 `Insert into terminal`이 있어, 같은 탭의 터미널에 붙여 넣는다(실행하지는 않는다). Ghostty의 붙여넣기 경로를 쓰므로 위험한 붙여넣기 확인이 그대로 적용되고, 내용은 클립보드에도 남는다.
- 링크: 웹(`https://`, `http://`, `mailto:`)은 기본 브라우저로, 다른 Markdown 파일은 같은 Pane에서, 그 밖의 로컬 파일은 Project explorer처럼 연다. 문서 안 앵커(`#…`)와 다른 스킴은 따르지 않는다.
- 이미지는 문서 옆의 로컬 파일만 보인다. 웹 이미지는 그 서버에 파일을 연 사실이 알려지지 않도록 불러오지 않고 `[image: 설명]`으로 표시한다.
- 파일이 디스크에서 바뀌면(다른 편집기에서 저장) 다시 읽는다. 열린 문서는 레이아웃과 함께 저장·복원된다.
- 위쪽 `Rendered | Raw`나 `Cmd+E`로 같은 Pane에서 Markdown 원문 편집기(Code editor)와 오간다. 저장하지 않은 변경이 있으면 Rendered로 돌아가기 전에 `Save` / `Don't Save` / `Cancel`을 묻는다. 스크롤 위치는 이어지지 않는다.

### Code editor

- Project explorer, Global search(해당 줄로), Code review의 `Open`에서 텍스트 파일(앞 8KB가 UTF-8이고 NUL이 없는 일반 파일)을 열면 bareframe 편집기로 연다. Settings › Features › Files의 `Open text files in the code editor`를 끄면 기본 앱으로 연다. 열리는 자리는 Markdown과 같은 `Choose a layout to open files`를 따르고, 이미 열린 파일은 그 Pane으로 이동한다. 열린 파일은 레이아웃과 함께 복원된다(저장하지 않은 변경은 남지 않는다).
- 줄 번호, 현재 줄 강조, 아래쪽 상태 줄(줄·열, 선택한 글자 수, 읽기 전용, LF/CRLF, UTF-8). 글꼴과 크기는 터미널 설정을 따른다. 줄바꿈 없이 가로로 스크롤한다.
- 키: 방향키(`Shift`로 선택), `Option+←/→` 단어, `Cmd+←/→`·`Home`/`End`·`Ctrl+A`/`Ctrl+E` 줄 처음(들여쓰기 → 0열)·끝, `Cmd+↑/↓` 문서 처음·끝, `Page Up/Down`, `Option+Backspace` 단어 지우기, `Cmd+Backspace` 줄 앞 지우기, `Enter`(들여쓰기 유지), `Tab`(파일이 탭을 쓰면 탭, 아니면 공백 4칸), `Esc` 선택 해제, `Cmd+A/C/X/V`(선택이 없으면 줄 전체 복사·잘라내기), `Cmd+Z`/`Cmd+Shift+Z`, `Cmd+S` 저장. 마우스: 클릭, 드래그 선택, `Shift`+클릭, 더블클릭 단어, 트리플클릭 줄.
- 한글 등 입력기 조합을 지원하고, 조합 중인 글자는 밑줄로 보인다. 한 음절 조합은 실행 취소 한 번으로 돌아간다. 타이핑은 단어 단위로, 지우기는 이어진 만큼 묶어서 되돌린다.
- 저장하지 않은 변경은 탭·Pane 제목에 `●`로 보인다. 저장하지 않은 채 Pane을 닫거나(`×`, `Cmd+W`) 창을 닫거나 `Cmd+Q`로 끝내면 편집기 위에 `Save` / `Don't Save` / `Cancel`을 묻는다. 끝내기·창 닫기는 저장하지 않은 편집기마다 차례로 물은 뒤 이어서 진행되고, `Cancel`을 누르면 멈춘다. 자동 저장은 디스크 변경 확인이 떠 있는 동안 저장하지 않는다.
- 다른 프로그램이 파일을 바꾸면 변경이 없을 때는 다시 읽고, 있으면 `Reload` / `Keep my changes`를 묻는다. 저장은 파일을 그 자리에서 덮어써 권한과 링크가 유지된다. 5MB가 넘는 파일과 한 줄이 64KB를 넘는 파일(압축된 JSON 등)은 읽기 전용으로 열고, 긴 줄은 64KB까지만 그린다. 64MB가 넘는 파일은 편집기로 열지 않고 기본 앱으로 넘긴다. 줄 끝이 섞인 파일은 더 많은 쪽(LF 또는 CRLF)으로 저장한다.
- 구문 강조: Rust, Markdown, TOML, JSON, YAML, Shell, Python, JavaScript/TypeScript/TSX, C/C++, Go, HTML, CSS, Swift, Java, Ruby, Lua, SQL, Dockerfile. 색은 터미널 테마 팔레트를 쓴다. 입력이 멈추고 잠시 뒤 백그라운드에서 다시 파싱하며, 그 사이 고친 줄만 잠깐 색이 빠진다. 2MB가 넘는 파일은 강조하지 않는다.
- 찾기 `Cmd+F`, 바꾸기 `Cmd+Option+F`: 편집기 위에 찾기 줄이 열리고, 한 줄 안의 선택은 그대로 찾을 말이 된다. 입력하는 동안 찾기를 시작한 곳 다음의 첫 결과로 간다. `Enter`·`↓` 다음, `Shift+Enter`·`↑` 이전(`Cmd+G`는 탭 그룹 키로 남겨 두었다), `Esc`·`×` 닫기. 대문자가 있으면 대소문자를 가리고 없으면 가리지 않는다. 결과 수와 현재 위치(`2 of 41`)를 보이고, 화면의 모든 결과를 칠한다. 바꾸기 칸의 `Enter`나 `Replace`는 현재 결과를 바꾸고 다음으로, `All`은 모두 바꾼다(실행 취소 한 번).
- 줄로 이동 `Ctrl+G`: 줄 번호를 넣고 `Enter`.
- Settings › Features › Editor: 줄 번호(절대·상대·끔), 자동 저장(기본 끔, 켜면 마지막 변경 1초 뒤 저장).
- 언어 서버(LSP), Warp처럼: Rust(`rust-analyzer`), Python(`pyright-langserver`), TypeScript/JavaScript(`typescript-language-server`), Go(`gopls`), C/C++(`clangd`)를 확장자로 고른다. 서버는 로그인 셸의 PATH에서 찾고(`~/.cargo/bin`, npm 전역 폴더 등), rustup처럼 프로젝트마다 툴체인이 다를 수 있어 프로젝트 위치에서 실제로 실행해 확인한다. 프로젝트는 파일을 담은 git 저장소(없으면 파일의 폴더)다.
  - 편집기 아래 줄: 설치돼 있지 않으면 `Install <서버>`(새 터미널 탭에서 설치 명령을 실행하고, 설치되면 알아서 `Enable`로 바뀐다), 설치돼 있으면 `Enable <서버>`. 켠 프로젝트는 설정 파일 옆 `language-servers`에 기억해 다음부터 그 프로젝트 파일을 열면 바로 시작한다.
  - 켜진 뒤에는 상태 점(초록 준비, 노랑 시작·작업 중, 빨강 실패)과 `<서버>: ready` 또는 진행 중인 작업, 오류·경고 수, `Restart`·`Disable`이 보인다. 서버가 끝나면 자동으로 다시 시작하지 않는다(Warp와 같음).
  - 오류는 빨간, 경고는 노란 물결 밑줄로 보인다. 고치는 동안은 지웠다가 서버가 다시 알려 주면 그린다. 마우스를 0.5초 멈추면 그 자리의 오류와 서버 설명(hover)이 카드로 뜨고, `Cmd`+클릭은 정의로 간다(다른 파일이면 그 파일을 연다). 저장하면 서버에 알려 rust-analyzer의 `cargo check` 같은 검사가 돈다.
  - 그 프로젝트의 파일을 모두 닫으면 서버도 끝난다. 서버의 에러 출력은 임시 폴더의 `bareframe-<서버>.log`에 남는다. 저장 시 자동 포맷, 참조 찾기, 자동 완성은 아직 없다.
- 파일 탭: 활성 탭에 편집기 Pane이 있으면 새로 여는 파일은 그 Pane의 파일 탭으로 들어간다(Settings › Features › Editor › `Group files into single editor pane`, 기본 켬). 파일이 둘 이상이면 위에 탭 줄이 보이고, 탭을 눌러 바꾸며 탭의 `×`로 그 파일만 닫는다. `Cmd+W`도 보이는 파일만 닫고, 마지막 파일을 닫으면 Pane이 닫힌다. Pane `×`는 모든 파일을 닫으며 저장하지 않은 파일마다 묻는다. Markdown 파일은 Rendered 전환 때문에 묶지 않는다. 열린 파일 목록과 보이던 파일은 레이아웃과 함께 복원된다.

### Code review

- 상단 바 오른쪽(톱니 왼쪽)에 Warp처럼 diff 버튼이 늘 있다. 활성 Pane의 작업 디렉터리가 git 저장소 안이고 줄이 바뀌었으면 아이콘 옆에 `+N −M`(HEAD 대비 추가·삭제 줄 수, 추적하지 않는 파일 포함)이 붙고, 저장소 밖이면 흐리게 보인다. 누르면 창 오른쪽에 Code review 패널이 열리고, 왼쪽 가장자리를 끌어 너비를 바꾼다. 패널의 열림과 너비는 레이아웃과 함께 저장된다. 버튼은 Settings › Features › Code review에서 끌 수 있다.
- 숫자와 패널은 명령이 끝날 때, 디렉터리를 옮길 때, 저장소 안 파일이 바뀔 때(1초 안에 몰린 변경은 한 번에) 다시 읽는다.
- 머리: `저장소: 브랜치`, 비교 대상(`Uncommitted changes` 또는 브랜치 — 고른 브랜치에서 갈라진 지점과 비교), 파일 수와 `+N −M`, `Commit`, `×`.
- 파일 카드: 변경 종류(M 수정, A 추가, D 삭제, R 이름 변경, U 추적 안 함, C 충돌), 이름과 폴더, `+N −M`, `Copy`(저장소 기준 경로 복사), `Revert`, `Open`. 카드를 누르면 diff를 접는다. 아래는 양쪽 줄 번호가 있는 통합 diff이고, 헝크마다 `Revert`가 있다. 이진 파일, 4MB 또는 한 줄 5000자를 넘는 diff는 보이지 않는다.
- `Revert`는 패널 안 확인 뒤에 실행한다. 파일: HEAD 내용으로 되돌린다(스테이징도 함께). 새 파일은 휴지통으로 옮긴다(되살릴 수 있음). 헝크: 그 헝크만 작업 트리에서 되돌린다. 브랜치와 비교하는 중에는 되돌리기를 숨긴다.
- `Commit`: 메시지를 쓰고 `Enter`나 `Commit`. 기본은 모든 변경이고, 파일 카드의 체크를 풀면 그 파일은 빠진다. 충돌이 있거나 merge·rebase·cherry-pick·revert가 진행 중이면 이유를 보이고 막는다. 저장소의 commit hook은 그대로 실행된다.
- 패널이 저장소를 읽을 때는 저장소 설정이 프로그램을 실행하지 못하게 fsmonitor, 외부 diff 도구, textconv를 끈다.

### 탭 그룹

- 여러 탭 선택(Warp와 같음): Shift+클릭은 활성 탭부터 클릭한 탭까지, Cmd+클릭은 한 탭씩 추가·제외한다. 일반 클릭은 선택을 풀고 그 탭으로 간다.
- 탭 우클릭 메뉴: 여러 탭을 고른 상태면 `Create group from tabs`, `Move to group ›`, `Remove from group`, `Close N tabs`. 한 탭이면 Warp 순서대로 구분선으로 나뉜 `Pin/Unpin tab`(그룹 밖 탭) / `New group with tab`, `Move to group ›`, `Remove from group` / `Copy pane title`, `Copy working directory` / `Rename tab`, `Move tab up/down`(같은 그룹·고정 영역 안에서) / `Close tab`, `Close other tabs`, `Close tabs above/below` / 탭 색상(없음과 6색, 고른 색으로 탭 카드가 물든다).
- 그룹 헤더 우클릭 메뉴: `Rename`, `New tab in group`, `Collapse/Expand group`, `Pin/Unpin group`, `Move group up/down`, `Ungroup tabs`, `Close all tabs in group`, `Close other tabs`.
- `Move to group ›`를 누르면 메뉴가 이동할 그룹 목록으로 바뀐다. 메뉴가 열린 동안 터미널은 정지 화면으로 바뀌어 메뉴가 터미널 위로 겹쳐도 가리지 않는다. `Esc`나 메뉴 밖 클릭으로 닫으면 키보드 포커스가 활성 Pane으로 돌아간다.

### 이름 변경과 고정

- 탭을 더블클릭하거나 메뉴의 `Rename tab`/`Rename`으로 이름을 바꾼다. 한글 입력이 되고, `Enter`로 확정, `Esc`로 취소, 다른 곳을 클릭하면 확정한다. 빈 이름으로 확정하면 탭은 셸이 알려 주는 제목으로, 그룹은 `New Group`으로 돌아간다. 이름을 고치지 않고 확정하면 탭은 계속 셸 제목을 따른다. 붙여 넣은 제어 문자는 공백으로 바뀐다.
- 고정한 탭과 그룹은 목록 맨 앞에 모이고, 일반 탭과는 구분선으로 나뉜다(핀 아이콘 표시). 고정한 탭에서 `Cmd+T`로 연 새 탭은 고정되지 않고 고정 영역 바로 뒤에 생긴다(고정 그룹 안이면 그룹을 따라 고정). 고정 그룹에서 빠진 탭은 고정이 풀린다. 그룹 위·아래 이동은 고정 영역을 넘지 않는다.
- `Cmd+G`/`Cmd+Shift+G`는 선택한 탭 전체(없으면 현재 탭)에 적용된다.

- 그룹 헤더는 색, 이름(기본 `New Group`), 둘째 줄의 `N tabs`로 표시되고, 그룹의 탭은 항상 붙어 있다.
- 헤더를 클릭하면 접고 편다. 접힌 그룹에 활성 탭이 있으면 헤더를 활성으로 표시한다.
- 헤더에 마우스를 올리면 `+`(그룹에 새 탭)와 `×`(그룹의 탭을 모두 닫기)가 나타난다.
- 그룹 안의 탭에서 `Cmd+T`를 누르면 새 탭도 같은 그룹에 들어간다.
- 탭을 끌면 Warp처럼 카드 전체가 커서를 따라오고 원래 자리는 빈 칸으로 남는다. 놓을 자리는 파란 줄 하나로 표시된다. 다른 탭 위에 놓으면 그 앞으로 옮겨지고 그 탭의 그룹에 들어간다. 그룹 헤더에 놓으면 그룹의 끝에, 목록 아래 빈 곳에 놓으면 그룹 밖 맨 끝으로 간다. 여러 탭을 선택한 상태로 끌면 함께 움직인다.
- 탭이 모두 빠진 그룹은 사라진다.
- Pane 경계를 끌어 크기를 조절한다. 클릭한 Pane이 활성 Pane이 된다.
- Pane이 둘 이상이면 각 Pane 위에 헤더가 붙는다. 가운데에 제목, 오른쪽에 `⋮`(오른쪽/아래 분할, 최대화) 메뉴와 `×`(Pane 닫기)가 있고, 활성 Pane은 헤더 왼쪽 위에 파란 삼각형이 표시된다.

## Settings

상단 바 오른쪽 톱니 버튼이나 `Cmd+,`로 Settings 탭을 연다. 왼쪽에 검색(모든 섹션의 항목 이름·설명에서 찾음)과 섹션 목록, 아래에 `Open settings file`이 있다.

| 섹션 | 항목 |
| --- | --- |
| Appearance — Themes | 테마: Dark / Light / Sync with OS(시스템 밝기 모드를 따름). 두 테마 모두 Claude 앱의 색(따뜻한 회색·아이보리, 클레이 주황 강조색)을 쓴다 |
| Appearance — Text | 터미널 폰트, 크기, 굵기(폰트에 그 이름의 스타일이 있을 때), 줄 높이 조정(%), 획 굵게, 최소 대비, 합자 |
| Appearance — Cursor | 커서 모양(Block / Bar / Underline), 깜박임 |
| Appearance — Panes | 터미널 안쪽 여백(px) |
| Appearance — Tabs | 새 탭에 현재 탭 색 유지 |
| Appearance — Tools panel | Project explorer, Global search 표시(하나만 켜면 도구 탭 줄을 숨기고, 둘 다 끄면 렌치 버튼도 숨김), Show hidden files(Project explorer의 `.` 파일·폴더, 기본 켬) |
| Features — General | 시작 시 창·탭·Pane 복원, 마우스 휠 스크롤 배율 |
| Features — Session | 새 세션의 셸(존재하는 절대 경로일 때), 새 세션의 작업 디렉터리(이전 세션 / 홈), 새 탭 위치(현재 탭 뒤 / 맨 끝) |
| Features — Keys | 왼쪽·오른쪽 Option을 Meta로 |
| Features — Terminal | 스크롤백 줄 수(제한 없음(기본) / 1,000 / 10,000 / 100,000 / 1,000,000. Ghostty는 따로 터미널마다 50MB에서 자르고, 새로 여는 터미널부터 적용), 마우스 리포팅, 선택 시 복사, 더블클릭 단어 경계 문자, 클립보드 접근(OSC 52: Deny / Ask / Allow, 기본 Deny. Ask는 확인 창에서 묻는다) |
| Keyboard shortcuts | 모든 동작의 단축키 목록과 검색(이름 또는 키, 예: `cmd d`). 줄을 누르면 새 단축키를 녹화하고 `Clear`(단축키 없음)·`Default`(기본값)·`Cancel`·`Save`. 다른 동작이 쓰던 키면 알려 주고, 저장하면 그 키를 이 동작으로 옮긴다. `Esc`는 녹화 취소 |
| About | 버전과 라이선스 |

- 화면에서 바꾼 값은 바로 적용되고 설정 파일에 그 줄만 고쳐 쓴다(주석과 다른 줄은 그대로, 기본값으로 돌리면 그 줄을 지움).
- 설정 파일 `~/.config/bareframe/config`(`$XDG_CONFIG_HOME`이 절대 경로면 그 아래)는 Ghostty처럼 `key = value` 형식이다. 직접 고쳐도 저장하는 순간 다시 읽어 적용한다. 잘못된 줄은 기본값을 쓰고 표준 에러에 경고만 출력한다.
- 터미널 항목은 Ghostty 설정으로 바뀌어 모든 Pane에 적용된다(예: `cursor-style = bar`, `terminal-padding = 8`은 `window-padding-x/y = 8`).

- 단축키 기본값은 Ghostty의 macOS 기본값을 따른다. 사이드바(`Cmd+B`)·Tools panel·탭 그룹(`Cmd+G`/`Cmd+Shift+G`)처럼 bareframe에만 있는 동작은 Warp를 따른다. 바꾼 단축키는 설정 파일에 Ghostty 문법의 `keybind` 줄로 저장되고, 직접 써도 된다(`keybind = cmd+shift+t=new_tab`, 해제는 `keybind = cmd+t=unbind`). 동작 이름은 Ghostty와 같다(예: `new_split:right`, `goto_tab:3`, `clear_screen`, `jump_to_prompt:-1`).
- Ghostty 기본값 중 새로 들어간 단축키: `Cmd+[`/`Cmd+]`(이전·다음 Pane), `Cmd+Ctrl+=`(Pane 크기 균등), `Ctrl+Tab`/`Ctrl+Shift+Tab`(다음·이전 탭), `Cmd+Alt+W`(탭 닫기), `Cmd+A`(전체 선택), `Cmd+K`(화면 지우기), `Cmd+Home`/`End`/`PgUp`/`PgDn`(스크롤), `Cmd+↑`/`↓`(이전·다음 프롬프트), `Cmd+Shift+,`(설정 파일 다시 읽기), `Cmd+Enter`·`Cmd+Ctrl+F`(전체 화면).

```text
theme = dark          # dark | light | system
font-family = Menlo   # 생략하면 Ghostty 기본 폰트
font-size = 13        # 6–72
cursor-style = bar    # block | bar | underline
```

| 키 / 메뉴 | 동작 |
| --- | --- |
| 메뉴바 | Warp의 구성(bareframe·File·Edit·View·Tab·Window)을 따르고, bareframe에 없는 Blocks·AI·Drive·Help는 뺐다. `Cmd+H`(숨기기), `Cmd+Alt+H`(다른 앱 숨기기), `Cmd+M`(최소화)도 macOS처럼 동작한다 |
| View > Dark Theme / Light Theme | 테마를 바꾸고 설정 파일에 저장 |
| `Cmd+=` / `Cmd+-` / `Cmd+0` | 모든 Pane의 폰트 크기 1pt 키우기 / 줄이기 / 설정 값으로 되돌리기(실행 중에만 유지) |

## 그 밖의 단축키와 종료

| 키 | 동작 |
| --- | --- |
| `Cmd+C` | 선택 영역을 클립보드로 복사 |
| `Cmd+V` | Ghostty의 붙여넣기 경로로 클립보드 입력. 위험한 붙여넣기(bracketed paste가 꺼진 프로그램에 여러 줄, 또는 bracketed paste를 빠져나가는 글자)는 확인 창(`Warning: Potentially Unsafe Paste`)에 붙여넣을 내용을 보여 주고 `Paste`를 눌러야 들어간다. `Esc`나 `Cancel`은 아무것도 넣지 않는다. 확인 창이 떠 있는 동안에는 다른 단축키와 창의 다른 곳 클릭이 막히고, 요청한 탭 번호와 이름·위치가 함께 보인다. 터미널 안 프로그램의 OSC 52 클립보드 읽기·쓰기는 Settings › Features의 `Clipboard access (OSC 52)`를 따른다: `Deny`(기본)는 거부, `Ask`는 같은 확인 창에서 `Allow`를 눌러야 허용, `Allow`는 묻지 않는다 |
| `Cmd+Q` | 앱 종료 |
| `Cmd+Shift+R` | 시작에 실패한 Pane을 다시 시작 |
| `Ctrl+C`, `Ctrl+D`, `Ctrl+Z` | 터미널과 셸에 전달 |

셸이 종료되면 그 Pane을 닫는다(마지막 탭의 마지막 Pane이면 새 탭으로 바꾼다). 시작에 실패한 Pane은 Pane 안에 오류와 재시작 방법을 표시한다.

## 실제 동작 검증

위 항목은 구현한 동작의 사용 계약이다. 현재 실행해 확인한 범위는 [MVP 1 검증 기록](mvp-1-validation.md)과 [MVP 2 검증 기록](mvp-2-validation.md)에 표시한다.
한글 IME와 `vim`, `less`, `htop`, `ssh`는 자동 모델 테스트와 별도로 실제 앱에서 확인한다.
