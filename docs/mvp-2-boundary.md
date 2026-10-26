# MVP 2 확장 경계

MVP 1은 일반 터미널을 구현하면서 Workspace, 세션 상태와 native backend의 의존성을 나눈다.
후속 Block Engine은 Ghostty의 화면 위젯 내부에 저장하지 않고, 별도 모델과 출력 보존 규약으로 구성한다.

## 현재 연결 구조

```text
native Ghostty callback
    → bounded observation queue
    → GhosttyAdapter
    → TerminalEvent with SessionKey
    → SessionState and WorkspaceView event stream
    → 후속 Block Engine
```

`bareframe-core`의 `TerminalEvent`는 GPUI와 Ghostty 타입을 포함하지 않는다.
각 이벤트는 `SessionId`와 실행 세대로 구성된 `SessionKey`를 가진다.
Pane은 세션 식별자를 참조하고, 재시작은 Pane·세션 식별자를 유지하면서 실행 세대를 바꾼다.
이전 실행에서 도착한 종료·메타데이터 이벤트는 새 세션 상태에 적용하지 않는다.

현재 어댑터에서 전달하는 이벤트는 세션 시작·종료, 제목, 작업 디렉터리, 명령 종료와 이벤트 유실 알림이다.
제목·디렉터리·명령 종료 알림은 셸이나 프로그램이 대응하는 터미널 제어 시퀀스를 보낼 때 발생한다.
clean zsh를 실행하는 것만으로 모든 메타데이터가 생성된다고 가정하지 않는다.

## 세션 종료와 명령 종료

`Exited`는 터미널을 소유한 셸의 수명이 끝났음을 나타낸다.
`CommandFinished`는 셸 안의 한 명령이 종료되었다는 관찰이며, 터미널 세션을 종료시키지 않는다.
현재 backend는 셸의 정확한 종료 코드를 항상 제공하지 않으므로 `Option<i32>`로 표현한다.

native 관찰 큐는 32개 이벤트와 이벤트당 4095바이트의 텍스트를 수용한다.
큐가 넘치거나 텍스트가 한도를 초과하면 유실된 이벤트 수를 `ObservationGap`으로 전달한다.
후속 Block Engine은 이 신호를 받으면 해당 구간을 불완전한 기록으로 취급해야 한다.

## MVP 2에서 추가할 계약

현재 `CommandFinished`만으로 명령 Block을 만들 수는 없다. 다음 작업이 남아 있다.

- 셸 hook이 전달하는 prompt·command start·command end와 명령 문자열의 규약.
- PTY read 경계에 걸쳐 들어오는 제어 시퀀스와 이벤트의 순서 보존.
- 명령 이벤트와 primary screen 출력의 정확한 대응 관계와 보존 방식.
- Alternate Screen 전환 중의 Block 처리.
- 취소·셸 종료·SSH·중첩 셸·hook 부재·이벤트 유실의 상태 정의.

현재의 raw PTY byte stream과 화면 내용은 Ghostty 내부에 있다.
MVP 2에서는 내부 어댑터에 추가 관찰 지점을 노출하거나 출력 스냅샷 API를 검증해야 한다.
화면의 문자열 차이만으로 명령별 출력을 복원하는 방식은 기본 계약으로 삼지 않는다.

## Workspace 확장

`WorkspaceState`는 Pane과 세션의 대응 관계만 관리하며 native surface를 보관하지 않는다.
현재 `PaneKind`에는 Terminal만 있다. 이후 편집기·Diff·브라우저를 추가할 때 각 도구가 자신의 자원을 소유하게 한다.
레이아웃 직렬화와 실행 중인 PTY 프로세스의 재개는 별도 기능으로 정의한다.
