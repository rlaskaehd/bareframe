# 사용 안내

빌드 준비가 끝나면 다음 명령으로 실행한다.

```sh
./script/run --clean-shell
./script/run --cwd ~/my-project
```

일반 실행은 `/bin/zsh -l -i`, clean shell은 `/bin/zsh -f -i`를 사용한다.
clean shell은 사용자의 zsh 초기화 파일을 읽지 않아 프롬프트 확장과 플러그인의 영향을 줄인다.
시스템의 `/etc/zshenv`와 상속한 환경 변수는 계속 적용될 수 있다.
사용자 zsh·Ghostty 설정 파일을 수정하지 않는다.

## 옵션

| 옵션 | 동작 |
| --- | --- |
| `--cwd DIRECTORY` | 시작 작업 디렉터리. 앱 바이너리 기준 기본값은 실행 시 현재 디렉터리 |
| `--shell EXECUTABLE` | 절대 경로로 지정한 셸 실행 파일. 기본 `/bin/zsh` |
| `--clean-shell` | zsh의 사용자 초기화 파일을 생략 |
| `--help`, `--version` | CLI 안내와 버전 표시 |

MVP 1의 셸 실행 모드는 zsh를 기준으로 한다. 다른 셸을 지정한 경우에도 `-l -i` 플래그를 사용하므로 해당 셸의 호환성을 확인해야 한다.

## 셸 통합

일반 zsh 실행은 앱이 배포한 zsh 통합을 `ZDOTDIR`로 불러온다. 통합 스크립트가 원래 `ZDOTDIR`을 복원한 뒤 사용자 초기화 파일을 평소처럼 읽고, 명령 경계(OSC 133)와 작업 디렉터리(OSC 7)를 앱에 알린다.
커서 모양, 창 제목 설정, `PATH` 추가 기능은 끈다. Ghostty 자체의 셸 감지·주입은 쓰지 않는다.

- 통합 파일은 `./script/bundle`이 `Contents/Resources/shell-integration`에, `./script/run`이 `target/debug/shell-integration`에 설치한다. GPLv3 파일이며 같은 디렉터리의 `COPYING`, `README`에 출처와 라이선스를 둔다.
- `--clean-shell`, zsh가 아닌 셸, 통합 디렉터리가 없는 실행은 통합 없이 일반 터미널로 동작한다.
- 앱을 실행한 터미널에서 상속한 `GHOSTTY_RESOURCES_DIR`, `GHOSTTY_SHELL_FEATURES`, `GHOSTTY_ZSH_ZDOTDIR`, `TERMINFO`는 시작할 때 지운다. cmux나 Ghostty 안에서 실행해도 `open`으로 실행한 것과 같게 동작하도록 하기 위해서다.

디버그 빌드에서 `BAREFRAME_VALIDATION=1`(검증 번들의 기본값, clean shell) 또는 `login`(사용자 로그인 셸)을 주면 `target/validation/`에 입력 trace와 TerminalEvent trace(`terminal-events.txt`)를 남긴다.
검증 번들을 로그인 셸로 띄우려면 `open -n target/bareframe-validation.app --env BAREFRAME_VALIDATION=login`을 쓴다.

## 단축키와 종료

| 키 | 동작 |
| --- | --- |
| `Cmd+C` | 선택 영역을 클립보드로 복사 |
| `Cmd+V` | Ghostty의 붙여넣기 경로로 클립보드 입력 |
| `Cmd+W` | 현재 창과 소유한 터미널 닫기 |
| `Cmd+Q` | 앱 종료 |
| `Cmd+Shift+R` | 종료되었거나 시작에 실패한 세션을 초기 작업 디렉터리에서 다시 시작 |
| `Ctrl+C`, `Ctrl+D`, `Ctrl+Z` | 터미널과 셸에 전달 |

실행 중인 세션에 재시작 단축키를 눌러도 현재 셸을 강제로 교체하지 않는다.
셸이 종료되면 마지막 화면을 유지하고 상태 영역에 종료와 재시작 방법을 표시한다.
셸 종료 코드를 얻지 못한 경우 코드 없이 종료 상태만 표시한다.

## 실제 동작 검증

위 항목은 구현한 동작의 사용 계약이다. 현재 실행해 확인한 범위는 [검증 기록](mvp-1-validation.md)에 표시한다.
한글 IME와 `vim`, `less`, `htop`, `ssh`는 자동 모델 테스트와 별도로 실제 앱에서 확인한다.
