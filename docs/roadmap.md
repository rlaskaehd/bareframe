# 구현 순서와 완료 기준

현재 상태: MVP 1 완료(2026-10-26, 태그 `mvp-1`), MVP 2 완료(2026-10-26, 태그 `mvp-2`, [검증 기록](mvp-2-validation.md)). MVP 1의 세부 결과와 남은 한계는 [검증 기록](mvp-1-validation.md)을 따른다.

## 기술 검증 — GPUI + libghostty

- `gpui-libghostty`가 사용하는 GPUI revision과 vendored Ghostty를 기준으로 최소 예제를 검토한다.
- Rust, Zig, Xcode 도구 요구 사항을 확인하고 호환 버전을 고정한다.
- 단일 GPUI 창에서 Ghostty surface 표시, 포커스, 키 입력, 출력, 리사이즈를 검증한다.
- PTY와 셸을 소유하는 계층을 정하고, Block Engine에 명령 lifecycle과 출력 경계를 전달할 방법을 조사한다.

완료 기준: 재현 가능한 빌드·실행 절차와 지원 버전을 기록하고, Block Engine에 필요한 이벤트를 얻을 경로를 설명할 수 있다.

## MVP 1 — 일반 터미널

상세 실행 순서는 [MVP 1 구현 계획](mvp-1-plan.md)을 따른다. 환경 준비부터 최소 구동, 입력·화면, 세션 종료, 수용 검증까지 M0–M6으로 나눈다.

- 단일 창, 단일 zsh 세션.
- 키보드·마우스 입력, 출력, 스크롤, 리사이즈.
- 터미널 세션 종료와 자원 해제.
- Alternate Screen 및 일반 TUI 동작.

완료 기준: `vim`, `less`, `htop`, `ssh`의 입력과 화면 전환을 직접 확인하고, 리사이즈와 프로세스 종료 후 정상 상태를 확인한다.
실제로 실행하지 않은 프로그램의 호환성을 확인 완료로 기록하지 않는다.

## MVP 2 — Warp 스타일 UI

상세 계획은 [MVP 2 구현 계획](mvp-2-plan.md)을 따른다.

- 왼쪽 사이드바의 세로 탭과 탭 그룹.
- 탭과 분할 Pane, 독립 터미널 세션 여러 개, 활성 Pane 포커스.
- 테마와 타이포그래피(터미널·UI 색상, 폰트, 크기).

완료 기준: 여러 탭과 분할 Pane에서 각 세션이 독립적으로 동작하고, 테마와 폰트 변경이 모든 Pane과 UI에 일관되게 적용된다.
MVP 1 수용 행렬의 입력·IME·TUI·종료 항목이 분할 Pane 안에서도 유지된다.

제외: 하단 고정 입력 편집기, 명령 Block. 커맨드 팔레트는 기능이 늘어난 뒤 추가한다.

## MVP 3 — Workspace

- Sidebar.
- Workspace 레이아웃, 작업 디렉터리 등 저장 가능한 상태의 저장과 복원.
- 커맨드 팔레트.
- MVP 2에서 넘어온 것: 위험한 붙여넣기·OSC 52 읽기 확인 창(지금은 거부), 메뉴·단축키로 바꾼 테마·폰트 저장, OSC 7 경로 신뢰 범위 재검토.

완료 기준: 앱 재실행 후 저장한 탭·분할 구성을 복원한다.
종료된 PTY 프로세스 자체의 재개 여부는 별도 기능으로 정의한다.

## 보류 — 명령 Block

Shell Integration 기반 명령 Block(명령·출력 연결, 접기, 재실행)은 현재 로드맵에서 제외했다.
조사 내용은 [명령 Block 확장 경계](block-boundary.md)에 남겨 둔다.

## MVP 4 — 개발 도구 확장

- Code Editor Pane.
- Git Diff Viewer Pane.
- File Explorer.
- WKWebView 기반 Browser Pane.

완료 기준: 하나의 Workspace에서 서로 다른 도구를 함께 배치하고, 포커스와 파일·경로 전달이 정상 동작한다.
AI Agent 통합은 이 구조 위의 후속 범위로 두고, 구체적인 요구가 정해지면 계획을 추가한다.
