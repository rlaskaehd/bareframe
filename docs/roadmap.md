# 구현 순서와 완료 기준

현재 상태: MVP 1 완료(2026-10-26), MVP 2 준비 중. 세부 결과와 남은 한계는 [검증 기록](mvp-1-validation.md)을 따른다.

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

## MVP 2 — 명령 Block

- zsh Shell Integration과 명령 lifecycle 이벤트.
- 명령, 작업 디렉터리, 실행 시간, 종료 코드와 출력의 연결.
- 출력 복사, Block 접기, 명령 재실행.
- 일반 출력과 Alternate Screen 전환의 분리.

완료 기준: 연속 명령, 실패 명령, 여러 줄 명령, 긴 출력, 취소된 명령에서 Block 경계와 상태가 맞는다.
TUI 진입·종료 및 Shell Integration이 없는 세션에서도 터미널을 계속 사용할 수 있다.

## MVP 3 — Workspace

- 탭과 분할 Pane.
- 독립 터미널 세션 여러 개.
- Sidebar와 활성 Pane / 포커스 관리.
- Workspace 레이아웃, 작업 디렉터리 등 저장 가능한 상태의 저장과 복원.

완료 기준: 독립 세션을 배치·종료하고, 앱 재실행 후 저장한 구성을 복원한다.
종료된 PTY 프로세스 자체의 재개 여부는 별도 기능으로 정의한다.

## MVP 4 — 개발 도구 확장

- Code Editor Pane.
- Git Diff Viewer Pane.
- File Explorer.
- WKWebView 기반 Browser Pane.

완료 기준: 하나의 Workspace에서 서로 다른 도구를 함께 배치하고, 포커스와 파일·경로 전달이 정상 동작한다.
AI Agent 통합은 이 구조 위의 후속 범위로 두고, 구체적인 요구가 정해지면 계획을 추가한다.
