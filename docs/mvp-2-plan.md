# MVP 2 구현 계획 — Warp 스타일 UI

기준: `mvp-1` 태그(2026-10-26). 상태: U1–U6 구현과 검증 완료, 결과는 [MVP 2 검증 기록](mvp-2-validation.md). 로드맵의 범위는 [MVP 2](roadmap.md#mvp-2--warp-스타일-ui)를 따른다.

## 범위

| 포함 | 제외 |
| --- | --- |
| 왼쪽 사이드바의 세로 탭과 탭 그룹(사용자 피드백으로 상단 탭 바에서 변경) | 하단 고정 입력 편집기 |
| 탭: 새 탭, 닫기, 전환, 제목 | 명령 Block(보류) |
| 분할: 좌우·상하 분할, 포커스 이동, 경계 드래그로 크기 조절 | 커맨드 팔레트(MVP 3) |
| 테마: 터미널 색상과 UI 색상을 한 세트로 전환 | 레이아웃 저장·복원(MVP 3) |
| 타이포그래피: 터미널 폰트·크기, UI 폰트 | Sidebar(MVP 3) |

## 사용 흐름과 단축키

| 키 | 동작 |
| --- | --- |
| `Cmd+T` | 새 탭(현재 Pane의 작업 디렉터리에서 시작) |
| `Cmd+W` | 현재 Pane 닫기. 탭의 마지막 Pane이면 탭을 닫고, 마지막 탭이면 새 탭으로 바꾼다 |
| `Cmd+Shift+[` / `Cmd+Shift+]`, `Cmd+1`–`Cmd+8`, `Cmd+9` | 이전·다음 탭, 번호로 탭 선택, 마지막 탭 |
| `Cmd+D` / `Cmd+Shift+D` | 오른쪽 / 아래로 분할 |
| `Cmd+Option+방향키` | 해당 방향의 Pane으로 포커스 이동 |
| `Cmd+=` / `Cmd+-` / `Cmd+0` | 모든 Pane의 터미널 폰트 크기 키우기·줄이기·기본값 |
| View 메뉴 | 테마 선택 |

- 셸이 종료되면 그 Pane을 닫는다(Warp·Ghostty와 같다). MVP 1의 하단 상태 표시줄과 종료 후 재시작은 없앤다.
- 시작에 실패한 Pane은 Pane 안에 오류를 표시하고, `Cmd+Shift+R`로 다시 시작한다.
- 창 제목에는 활성 탭의 제목을 쓴다. 탭 제목은 셸이 보내는 제목, 없으면 작업 디렉터리 이름이다.

## 설계

### 레이아웃 모델 (`bareframe-core`)

GPUI 없이 테스트할 수 있는 순수 모델로 둔다.

```text
Workspace
├── tabs: Vec<Tab>, active_tab
└── Tab
    ├── layout: Layout          Pane(PaneId) | Split { axis, ratio, first, second }
    └── active_pane: PaneId
```

- 분할은 이진 트리로 한다. 분할하면 현재 Pane 자리에 `Split`이 들어가고, 닫으면 형제 노드가 그 자리를 대신한다.
- 방향 포커스 이동은 각 Pane의 사각형(단위 정사각형 기준)을 계산해, 그 방향으로 가장 가까운 Pane을 고른다.
- `ratio`는 0.1–0.9로 제한한다.

### Pane과 세션 (앱)

- `TerminalPane` entity가 `SessionState` 하나와 `GhosttyAdapter` 하나를 소유한다. 지금 `WorkspaceView`가 가진 세션 수명 로직을 그대로 옮긴다.
- `WorkspaceView`는 레이아웃 모델과 `PaneId → TerminalPane` 표를 가진다. native surface는 소유하지 않는다.
- Ghostty surface는 GPUI 위에 얹힌 NSView라서 렌더 트리에서 빠져도 화면에 남는다. 그래서 탭을 바꿀 때 숨는 탭의 Pane은 `set_visible(false)`, 보이는 탭의 Pane은 `set_visible(true)`로 명시한다.
- 같은 이유로 GPUI 요소를 surface 위에 겹쳐 그릴 수 없다. 활성 Pane 표시는 surface 바깥 테두리(여백)에 그린다.

### 탭 바와 창

> 첫 구현 뒤 사용자 피드백으로 상단 탭 바를 왼쪽 사이드바로 옮기고 탭 그룹을 추가했다. 아래는 첫 설계이며, 현재 동작은 [사용 안내](usage.md#탭과-분할)를 따른다.

- `TitlebarOptions { appears_transparent: true, traffic_light_position }`로 시스템 타이틀바를 숨기고 그 높이에 탭 바를 그린다.
- 탭 바의 빈 영역은 `WindowControlArea::Drag`로 창 이동, 더블클릭은 `titlebar_double_click`으로 확대.
- 탭: 제목, 마우스를 올리면 닫기 버튼, 오른쪽 끝의 `+` 버튼. 활성 탭은 배경으로 구분한다.

### 테마와 타이포그래피

- `Theme` 하나가 UI 색상(배경, 탭, 경계, 텍스트, 강조)과 터미널 색상(배경, 전경, 16색)을 함께 가진다. 내장 테마는 Dark(기본)와 Light 두 개로 시작한다.
- 터미널 쪽은 vendor의 `TerminalTheme` 설정 파일 경로를 쓴다. 이 파일에 `font-family`, `font-size` 줄을 함께 넣을 수 있게 vendor를 최소한으로 바꾼다. 실행 중 변경은 기존 `TerminalThemeState::apply`(`ghostty_surface_update_config`)로 모든 Pane에 적용한다.
- UI 폰트는 시스템 UI 폰트(`.SystemUIFont`)를 쓴다.
- 설정 파일 `~/.config/bareframe/config`(Ghostty와 같은 `key = value` 형식)에서 `theme`, `font-family`, `font-size`를 읽는다. 파일이 없거나 값이 잘못되면 기본값을 쓰고 경고만 출력한다. validation instance는 이 파일을 읽지 않는다.
- 메뉴에서 바꾼 테마와 폰트 크기는 실행 중에만 유지한다. 저장은 MVP 3에서 레이아웃 저장과 함께 다룬다.

## 단계

| 단계 | 목표 | 완료 기준 |
| --- | --- | --- |
| U1 | 레이아웃 모델 | 분할·닫기·형제 승격·방향 포커스·탭 추가/닫기/전환을 단위 테스트로 확인 |
| U2 | 다중 Pane과 세션 | `TerminalPane` 분리, 여러 세션 동시 실행, 셸 종료 시 Pane 닫기, 탭 전환 시 surface 숨김 |
| U3 | 탭 바 | 투명 타이틀바, 탭 표시·전환·닫기·새 탭, 창 이동과 더블클릭 확대, 탭 단축키 |
| U4 | 분할 | 분할 단축키, 방향 포커스, 활성 Pane 테두리, 경계 드래그로 크기 조절 |
| U5 | 테마·타이포그래피 | 설정 파일, Dark/Light, View 메뉴 전환, 폰트 크기 단축키가 모든 Pane과 UI에 적용 |
| U6 | 수용 검증과 문서 | 아래 시나리오를 실제 앱에서 실행하고 `docs/mvp-2-validation.md`, `docs/usage.md` 갱신 |

U1과 U5의 설정 파싱은 화면 없이 테스트한다. U2–U4는 단계마다 `./script/check`와 실제 앱 실행으로 확인한다.

## 수용 검증 시나리오

| 영역 | 시나리오 | 기대 결과 |
| --- | --- | --- |
| 탭 | 탭 3개 생성, 번호·이전·다음 전환, 가운데 탭 닫기 | 각 탭의 셸이 독립적이고, 숨은 탭의 화면이 겹쳐 보이지 않음 |
| 분할 | 좌우·상하 중첩 분할 4개, 방향 포커스, 경계 드래그 | 입력이 활성 Pane에만 가고, 각 Pane의 `stty size`가 크기와 일치 |
| 닫기 | Pane에서 `exit`, `Cmd+W`, 마지막 Pane·마지막 탭 닫기 | 형제 Pane이 자리를 채우고, 마지막이면 창이 닫히며 셸 프로세스가 남지 않음 |
| TUI | 분할 Pane에서 vim·htop 실행 후 크기 조절 | 화면이 Pane 크기에 맞게 다시 그려짐 |
| IME | 분할 Pane 사이를 오가며 한글 입력 | 조합이 활성 Pane에만 표시되고 다른 Pane으로 새지 않음 |
| 테마 | Dark ↔ Light 전환 | 모든 Pane의 터미널 색상과 탭 바·경계가 함께 바뀜 |
| 폰트 | 설정 파일의 폰트·크기, `Cmd+=`/`-`/`0` | 모든 Pane에 적용되고 셸의 행·열 수가 다시 계산됨 |
| 창 | 탭 바 드래그 이동, 더블클릭 확대, 리사이즈 | 창 동작이 일반 macOS 창과 같음 |

## 위험

| 항목 | 확인할 내용 | 대응 |
| --- | --- | --- |
| native surface 여러 개 | Pane마다 Ghostty app·config가 생기는 메모리와 시작 시간 | U2에서 Pane 8개로 측정. 문제가 되면 app 공유를 vendor에서 검토 |
| 숨김 처리 | 탭 전환·분할 직후 surface가 잘못된 위치에 남는지 | 보이는 Pane만 `set_visible(true)`, 프레임은 매 레이아웃마다 갱신 |
| 키 바인딩 충돌 | `Cmd+D` 등 앱 단축키가 터미널로도 전달되는지 | 앱 action으로 처리되면 surface에 보내지 않음을 U3·U4에서 확인 |
| 폰트 변경과 PTY 크기 | 폰트 크기 변경 후 행·열이 PTY에 반영되는지 | U5에서 `stty size`로 확인 |
