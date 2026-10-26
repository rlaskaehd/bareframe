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

## MVP 3 — 도구 패널, 설정, 편집 도구

상세 계획은 [MVP 3 구현 계획](mvp-3-plan.md)을 따른다(계획 단계).

- **MVP 3a**: MVP 2에서 넘어온 항목(위험한 붙여넣기 확인 창, 설정 저장, OSC 7 재검토), Tools panel(Project explorer·Global search), 톱니 아이콘의 Settings(텔레메트리·계정·AI 항목 제외)와 Keyboard shortcuts(기본값은 Ghostty, 수정 가능).
- **MVP 3b**: Markdown viewer·편집, Diff 버튼과 Code review 창, Code editor.
- 원래 MVP 3에 있던 레이아웃 저장·복원과 커맨드 팔레트의 위치는 계획의 "결정이 필요한 것"에서 정한다.

완료 기준: 3a는 파일 탐색·검색과 설정·단축키 변경이 재시작 후에도 유지되는 것, 3b는 같은 작업 영역에서 터미널과 Markdown·편집기·Diff를 함께 쓰는 것.

## 보류 — 명령 Block

Shell Integration 기반 명령 Block(명령·출력 연결, 접기, 재실행)은 현재 로드맵에서 제외했다.
조사 내용은 [명령 Block 확장 경계](block-boundary.md)에 남겨 둔다.

## MVP 4 — 개발 도구 확장

- WKWebView 기반 Browser Pane.
- Code Editor·Git Diff·File Explorer는 MVP 3으로 옮겼다.

완료 기준: 하나의 Workspace에서 서로 다른 도구를 함께 배치하고, 포커스와 파일·경로 전달이 정상 동작한다.
AI Agent 통합은 이 구조 위의 후속 범위로 두고, 구체적인 요구가 정해지면 계획을 추가한다.
