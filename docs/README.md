# bareframe 개발 문서

bareframe은 **Rust + libghostty를 기반으로 하는 Warp 스타일의 모듈형 개발 워크스페이스 터미널**을 목표로 한다.
터미널을 중심으로 탭과 분할 패널, 명령 Block, 파일 탐색기, Git Diff, 편집기, 브라우저를 단계적으로 통합한다.

MVP 1(단일 창·단일 zsh 터미널)을 완료했다. 남은 한계는 [검증 기록](mvp-1-validation.md)에 있으며, 다음 단계는 MVP 2 명령 Block이다.

## 문서

- [아키텍처와 기술 선택](architecture.md): Workspace Core, Terminal Core, Block Engine의 책임과 검증할 사항.
- [구현 순서와 완료 기준](roadmap.md): 기술 검증부터 MVP 1–4까지의 범위.
- [MVP 1 구현 계획](mvp-1-plan.md): 의존성 고정, 단일 zsh 터미널 구현 단계와 수용 검증.
- [빌드 안내](build.md), [사용 안내](usage.md), [검증 기록](mvp-1-validation.md).
- [MVP 2 확장 경계](mvp-2-boundary.md): 이벤트, 실행 세대, 명령 종료와 출력 보존의 책임.
- [MVP 2 구현 계획](mvp-2-plan.md): 셸 통합 주입, 명령 경계 API, Block 모델·패널 단계와 수용 시나리오.
- [Git 전략](git-strategy.md): 브랜치·커밋·PR·vendor·태그 규칙과 세션별 역할.
- [오픈소스 참조 안내](../reference/README.md): 내려받은 저장소의 역할, 주요 경로, 라이선스 표시.
- [소스 잠금 정보](../reference/sources.lock.json): 공식 URL, 커밋, 소스 압축 파일의 SHA-256, 무결성 검증 기록.

## 현재 결정

- 독립적인 Rust 애플리케이션을 만들고 GPUI + libghostty 조합부터 검증한다.
- Terminal Core와 Block Engine, Workspace Core의 상태와 책임을 분리한다.
- Warp는 명령 Block과 워크스페이스 설계를 살펴보는 참조 자료로 사용한다.
- 최초 구현은 macOS에서 단일 창과 zsh 세션을 구동하는 데 집중한다.
- 편집기, Diff Viewer, 파일 탐색기, 브라우저, AI Agent는 이후 단계에서 독립적인 Pane으로 확장한다.

2026-10-26 기준의 초기 방향이며, 기술 검증 결과에 따라 세부 라이브러리와 API 버전을 확정한다.
