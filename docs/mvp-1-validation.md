# MVP 1 검증 기록

상태: **MVP 1 완료.** 수용 행렬에 FAIL은 없다. 다음은 기록된 한계로 받아들였다.

- NOT_RUN: native 초기화 실패(재현 수단 없음), Retina 배율·다른 배율 모니터·최소화(하드웨어·범위 밖).
- 셸 실행 실패가 `Failed`가 아니라 세션 종료로 표시되는 차이(아래 기록).
2026-10-26, macOS 26.7.1(25G241) arm64, Rust 1.98.1, zsh 5.9에서 수행한 결과다.

| 검사 | 결과 | 범위 |
| --- | --- | --- |
| 코어 fmt | PASS | backend 독립 Rust 코드 |
| 코어 Clippy | PASS | `--all-targets -- -D warnings` |
| 코어 테스트 | PASS | 현재 12개. 세션 재시작, 상태 전이, CLI, UTF-16 조합, IME 확정 보류와 Backspace |
| C 관찰 큐 테스트 | PASS | FIFO, UTF-8, 명령 종료 코드·시간, overflow, 큰 메타데이터 |
| Objective-C shim | PASS | clang `-fsyntax-only -Wall -Wextra -Werror` |
| 데스크톱 Rust fmt | PASS | `cargo fmt -- --check` |
| Cargo manifest | PASS | `cargo metadata --no-deps` |
| 데스크톱 의존성 취득·빌드 | PASS | `cargo build --locked`, 기존 `Cargo.lock` 사용 |
| 데스크톱 Clippy·테스트 | PASS | `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`(앱 crate 테스트 0개) |
| vendor `gpui-libghostty` 테스트 | PASS | `cargo test --locked -p gpui-libghostty --lib` 18개. workspace 밖이라 `./script/check`에 포함되지 않음 |
| 실제 앱 창·zsh | PASS | 아래 [GUI 수용 검증](#gui-수용-검증) |
| 입력(modifier·반복·Tab·포커스) | PASS | 수정 후 재검증. 아래 [FAIL 2건 수정 후 재검증](#fail-2건-수정-후-재검증) |
| 한글 IME | PASS | 수정 후 재검증. 아래 [FAIL 2건 수정 후 재검증](#fail-2건-수정-후-재검증) |
| 클립보드 | PASS | 선택 복사, 여러 줄 붙여넣기, bracketed paste |
| 리사이즈·스크롤 | PASS | `stty size`, TUI 재배치, scrollback |
| Retina 배율·최소화·다른 배율 모니터 | NOT_RUN | 이번 범위에서 실행하지 않음 |
| vim·less·htop·ssh | PASS | ssh는 로컬 fixture |
| 창 닫기·Cmd+W·Cmd+Q 시 자식 프로세스 SIGHUP | PASS | 아래 [종료 시 프로세스 정리](#종료-시-프로세스-정리) |
| `exit`·`Ctrl+D`·세션 20회 반복 | PASS | 남은 셸·login 프로세스 없음 |
| 오류(잘못된 cwd·실행할 수 없는 셸) | PASS | CLI 실행 기준. native 초기화 실패는 NOT_RUN |

`./script/check`(FAIL 수정 후 14:47 완료, 종료 코드 0)가 코어 검사, fmt, Clippy, 테스트, `cargo build --locked`를 순서대로 실행했다.
`Cargo.lock`은 앱 전체 의존성 해석 결과로 저장소에 있으며, 모든 cargo 명령은 `--locked`로 실행했다.
Cargo는 `block v0.1.6`, `proc-macro-error2 v2.0.1`에 대해 future-incompatibility 경고를 출력했지만 빌드와 검사는 실패하지 않았다.

## GUI 수용 검증

2026-10-26 14:00–14:23, `./script/bundle --validation`으로 다시 만든 `target/bareframe-validation.app`(clean shell `zsh -f -i`)에서 수행했다.
기본 실행 항목만 `target/debug/bareframe`을 인자 없이 실행해 사용자 zsh 설정(로그인 셸)에서 확인했다.
근거 파일은 `target/validation/run2/`에 있고, 이번 실행에서 새로 만든 파일만 판정 근거로 사용했다.
기존 `target/validation/`의 `vim.txt`, `ssh-vim.txt`, `job-control.txt`, `restart.log` 등은 생성 경위가 기록되어 있지 않아 근거로 쓰지 않고 해당 항목을 다시 실행했다.

| 영역 | 시나리오 | 결과 | 확인 내용 |
| --- | --- | --- | --- |
| 실행 | 기본 실행(인자 없음) | PASS | `pwd` = 실행 디렉터리, `$0` = `-/bin/zsh`, 로그인·대화형 셸, `TERM=xterm-256color`, `COLORTERM=truecolor` |
| 실행 | `--cwd` + 공백·한글 경로 | PASS | `target/validation/경로 테스트 dir`에서 `pwd` 일치, 상태 표시줄 경로 일치 |
| 셸 | 파이프·리다이렉션 | PASS | `printf … \| sort \| tr`, `>`, `>>`, `<` 결과 `abcappended` |
| 셸 | `sleep` 취소 | PASS | `sleep 30` 중 `Ctrl+C`, `$?` = 130 |
| 셸 | `Ctrl+Z`와 `fg` | PASS | `jobs` = `suspended sleep 40`, 프로세스 상태 `T`, `fg` 후 종료 코드 0, 남은 job 0 |
| 입력 | 일반 키와 modifier | PASS | 수정 전 FAIL(`Ctrl+Shift+A`, `Ctrl+Shift+2`, `Ctrl+Shift+-` 바이트 누락). 수정 후 `ESC[97;6u`, `ESC[64;5u`, `\x1f`로 Ghostty.app과 같고, 나머지 키도 수정 전과 같음([재검증](#fail-2건-수정-후-재검증)) |
| 입력 | 키 반복 | PASS | autorepeat 이벤트 `a`×10, `←`×5, Backspace×3 → 각각 첫 입력 + 10·5·3회 전달 |
| 입력 | 앱 전환 후 재입력 | PASS | Finder를 앞으로 가져온 뒤 창을 클릭하고 입력, 명령 결과 일치(누락·중복 없음) |
| 한글 | 조합 표시 | PASS | `ㅎ→하→한`, `ㄱ→그→글` 단계별 mark, 조합 중 문자를 밑줄로 표시 |
| 한글 | 확정 | PASS | `printf '%s\n' '한글 입력'` 결과 일치(입력 소스 단축키로 전환한 경우), 중복 확정 없음 |
| 한글 | Backspace | PASS | 수정 전 FAIL(남은 `ㅎ`에서 Backspace → PTY에 `ㅎ` 후 `0x7f`). 수정 후 `ㅎ`과 `0x7f` 모두 전달되지 않고 Ghostty.app과 같음([재검증](#fail-2건-수정-후-재검증)) |
| 한글 | 조합 취소(Esc) | PASS | 조합 중 Esc → `한` 확정 후 `ESC` 전달. 표시와 입력이 일치함 |
| 한글 | 영문 전환 | PASS | 조합 중 `Ctrl+Space`로 ABC 전환 → `한글` 확정 후 `x` 입력, 결과 `한글x` |
| 클립보드 | 선택 후 복사 | PASS | 마우스 드래그 + `Cmd+C`, 클립보드 = `SEL-한글-marker-42 end` |
| 클립보드 | 여러 줄 붙여넣기 | PASS | 두 줄 명령을 `Cmd+V` 후 Return, 두 명령 모두 실행되고 한글 보존 |
| 클립보드 | bracketed paste | PASS | `?2004h`에서 `ESC[200~A1\nB2 한글ESC[201~`, 해제 시 `A1\rB2 한글` |
| 화면 | 색상 | PASS | 16색·밝은 색·256색·truecolor 그라데이션, bold·italic·underline·reverse (`screen-render.png`) |
| 화면 | CJK·emoji·결합 문자 | PASS | `\|한글\|中文\|日本語\|` 열이 ASCII 줄과 정렬, `😀 👍🏽 🇰🇷`, `é ñ`, 넓은 문자 뒤 커서 위치 일치 |
| 화면 | 긴 출력·scrollback | PASS | 5,000줄 출력 후 휠로 `scroll-4790`까지 탐색, 이어서 입력한 명령 정상 실행 |
| 크기 | 리사이즈와 `stty size` | PASS | 창 드래그로 23×49 → 45×138 → 29×101 → 47×157 → 32×113, 매번 `stty size`와 일치 |
| 크기 | TUI 재배치 | PASS | vim의 `&lines`·`&columns`가 45x138 → 29x101로 갱신, htop이 커진 창 전체에 다시 그려짐 |
| vim | 열기·편집·저장·리사이즈·종료 | PASS | 저장 내용 `hello vim edit`, 종료 후 프롬프트와 `stty size` 정상 |
| less | 검색·스크롤·종료 | PASS | `/less-line-2500` 강조 표시, `g`·`j`·Space 이동, 종료 후 이전 화면 복원 |
| htop | 실행·선택·리사이즈·종료 | PASS | htop 3.5.3, 선택 행 이동, 리사이즈 후 전체 재배치, `q` 후 htop 프로세스 없음 |
| ssh | 접속·원격 명령·TUI·리사이즈·종료 | PASS | 원격 tty `/dev/ttys019`, 원격 vim 저장, window-change 47×157 → 32×113, `exit` 후 로컬 프롬프트 복귀 |
| 종료 | `exit` | PASS | 셸·login 종료, 제목 `Session ended`, 상태 `Session ended · Cmd+Shift+R to restart` |
| 종료 | 빈 프롬프트의 `Ctrl+D` | PASS | 셸 종료, 같은 상태 표시 |
| 반복 | 세션 시작·종료 20회 | PASS | `Cmd+Shift+R` → 명령 → `exit` 20회, 20개 셸 PID 모두 사라지고 앱 자식 프로세스 0개, 열린 `ptmx` 1개, 이후 새 세션 입력 정상 |
| 오류 | 잘못된 cwd | PASS | 없는 경로·파일 경로 모두 메시지 출력 후 종료 코드 2 |
| 오류 | 실행할 수 없는 셸 | PASS | 실행 권한 없음·상대 경로·없는 경로는 종료 코드 2. 실행 권한이 있으나 실행되지 않는 파일은 창에 Ghostty 실행 실패 메시지와 재시작 안내 표시 |
| 오류 | native 초기화 실패 | NOT_RUN | 실패를 재현할 방법이 없어 실행하지 않음 |

`exit 7`로 종료해도 상태 표시줄에 종료 코드가 나오지 않는다. 어댑터가 종료 코드를 제공하지 않기 때문이며, 알 수 없는 값을 `0`으로 표시하지 않는다는 계획과 일치한다.
실행되지 않는 셸은 앱 상태가 `Failed`가 아니라 `Session ended`로 표시된다. 오류 확인과 재시작은 가능하므로 행렬 기준은 충족하지만, M5의 "셸 실행 실패 구분" 기준과는 차이가 있다.
`open`으로 앱 번들을 실행하면서 잘못된 `--cwd`를 주면 창 없이 종료되고 오류는 stderr에만 남는다. 문서화된 실행 경로(`./script/run`)에서는 오류가 보인다.

### FAIL 2건 수정 후 재검증

처음 검증(14:00–14:23)에서 FAIL이었던 두 항목을 수정한 뒤, 같은 HID 도구로 14:38–14:47에 다시 확인했다.
앱은 수정 코드로 다시 만든 `target/bareframe-validation.app`이다. 근거 파일은 `target/validation/run2/review-*.txt`이며, 각 파일은 `keyrec.py`가 받은 PTY 입력 바이트다.
같은 키 입력을 Ghostty.app 1.3.1에도 보내 대조했다(`review-ghostty-*.txt`).

1. **`Ctrl+Shift+<키>` 입력 누락**
   - 수정 전 증상: `Ctrl+Shift+A`, `Ctrl+Shift+2`, `Ctrl+Shift+-`를 누르면 PTY에 아무 바이트도 전달되지 않았다(`keys3.txt`, `keys4-abc.txt`, `mods-hid.txt`).
   - 원인: 이전에 적은 두 후보는 아니었다. 이벤트는 `key_down`에 도착했고 `native_key()` 매핑도 성공했다. GPUI는 Control이 눌리면 `key_char`를 비워서 넘기는데, Ghostty의 legacy 인코더는 text가 없으면 C0 제어 문자와 Alt 접두만 처리하고 CSI u 경로에 도달하지 않는다(`vendor/.../ghostty/src/input/key_encode.zig`의 `utf8.len == 0` 분기). Ghostty.app은 Control을 뺀 문자를 text로 넘긴다.
   - 수정: 모든 키 입력이 지나는 `NativeSurface::send_key`(`vendor/gpui-libghostty/crates/gpui-ghostty/src/terminal.rs`)에서, Control이 있고 Super가 없으며 text가 없을 때 출력 가능한 ASCII 한 글자를 text로 보충한다(Shift면 대문자). `vendor/gpui-libghostty/BAREFRAME.md`에 기록했다.
2. **한글 조합의 마지막 자모 Backspace**
   - 수정 전 증상: 조합 중인 `ㅎ` 하나만 남은 상태에서 Backspace를 누르면 PTY에 `ㅎ` 다음 `0x7f`가 전달되었다(`bs-bf.txt`).
   - 원인: 한국어 IME는 마지막 자모에서 Backspace를 받으면 같은 키 이벤트 안에서 `ㅎ`을 확정(`insertText`)하고 Backspace를 앱으로 넘긴다. 어댑터가 확정 텍스트를 즉시 PTY로 보냈기 때문에 둘 다 전달되었다. Ghostty.app은 확정 텍스트를 같은 키 이벤트에 붙여 보내고, 인코더가 "Backspace + text"를 아무것도 보내지 않는 것으로 처리한다.
   - 수정: 확정 텍스트를 `Composition`(`crates/bareframe-core/src/ime.rs`)에 보관하고 키 이벤트가 끝난 뒤 보낸다(`src/terminal/adapter.rs`의 `replace_text_in_range`, `send_committed`). 같은 이벤트에서 Backspace가 오면 가장 최근 확정과 Backspace를 함께 버린다. 다른 키, 복사·붙여넣기 전에는 보관한 텍스트를 먼저 보낸다.

| 시나리오 | 입력 소스 | 키 | bareframe | Ghostty.app | 결과 |
| --- | --- | --- | --- | --- | --- |
| Ctrl+Shift 조합 | ABC | `Ctrl+Shift+A` `Ctrl+Shift+2` `Ctrl+Shift+-` `Ctrl+B` | `ESC[97;6u ESC[64;5u \x1f \x02` | 같음 | PASS |
| Ctrl+Shift 조합 | 2벌식 | `Ctrl+Shift+A` `Ctrl+Shift+2` `Ctrl+B` | `ESC[97;6u ESC[64;5u \x02` | `ESC[12609;6u ESC[64;5u \x02` | PASS(아래 차이 참고) |
| 일반 키·modifier 회귀 | ABC | `a` `Shift+B` `1` `Ctrl+A/E/C` Tab `Shift+Tab` ↑ ← Home End Backspace Forward Delete Esc F1 `Option+B` `Option+←` `Ctrl+←` `Shift+↑` `Ctrl+Option+A` | 수정 전 기록과 같음 | `Option+B`만 `ESC b`, 나머지 같음 | PASS |
| 마지막 자모 Backspace | 2벌식 | `g k s` ⌫⌫⌫ `r k` Space | `가 ` | `가 ` | PASS |
| 낱자 입력 후 Backspace 반복 | 2벌식 | `r` ⌫ `r` ⌫ `g k` Space | `하 ` | — | PASS |
| 확정 | 2벌식 | `g k s r m f` Space | `한글 ` | — | PASS |
| 조합 중 Backspace | 2벌식 | `d l q f u r` ⌫ Space | `입려 ` | — | PASS |
| 조합 중 Return | 2벌식 | `g k s` Return | `한\r` | — | PASS |
| 조합 중 Esc | 2벌식 | `g k s` Esc | `한` 후 `ESC` (수정 전과 같음) | — | PASS |
| 확정 후 Backspace | 2벌식 | `g k s` Space ⌫ | `한 \x7f` | — | PASS |
| 조합 중 `Cmd+V` | 2벌식 | `g k s` `Cmd+V`(클립보드 `P`) | `한P` | — | PASS |
| 조합 중 영문 전환 | 2벌식 → ABC | `g k s` `Ctrl+Space` (0.7초 대기) `x` | `한x` | — | PASS |
| 선택 복사 | ABC | 마우스 드래그 + `Cmd+C` | 클립보드 = 선택한 글자 | — | PASS |
| 조합 중 `Cmd+W`·`Cmd+Q` | 2벌식 | `g k s` 후 `Cmd+W` / `Cmd+Q` | 앱·login·zsh 종료, crash report 없음 | — | PASS |

- 2벌식에서 `Ctrl+Shift+<글자>`의 CSI u 코드가 Ghostty.app과 다르다. Ghostty.app은 한국어 자판의 글자(`ㅁ`, U+3141)를, bareframe은 GPUI가 넘기는 ASCII 자판 글자(`a`)를 쓴다. 영문 자판에서는 같으며, 셸의 키 바인딩에는 ASCII 쪽이 맞는다고 보고 PASS로 판정했다.
- `Option+B`의 차이(bareframe `∫`, Ghostty.app `ESC b`)는 대조에 쓴 Ghostty.app이 사용자 설정을 읽기 때문으로 보인다. bareframe의 결과는 수정 전 기록과 같다.
- `Ctrl+Space` 직후 대기 없이 `x`를 보내면 입력 소스가 바뀌지 않고 `x`가 `ㅌ`으로 조합된다. Ghostty.app에서도 똑같이 재현되므로(`review-ghostty-switch.txt`) 도구의 한계로 판정했다. 도구가 Control 해제 이벤트를 따로 보내지 않는 것이 원인으로 보인다. 0.7초를 기다리면 정상이다(`review-ko-switch2.txt`).
- 선택 복사는 드래그 시작점이 첫 글자 칸의 오른쪽 절반이어서 첫 글자를 뺀 `OPY-MARKER-77`이 복사되었다. 선택한 범위와 클립보드 내용은 일치한다.

### `copied.txt` 판정

`target/validation/copied.txt`의 두 줄은 이번 검증 전(12:52)에 기록되었다.

- `printf '%s\n' 'gks`: **조작 실수.** `input-trace.txt` 첫 줄이 조합 없이 `commit "g"`, `"k"`, `"s"`이다. 한국어 IME를 거쳤다면 `ㅎ`·`하`·`한` mark가 남는다. 2벌식 `g k s`(=`한`)를 키 대신 문자열로 보낸 결과와 같은 형태이며, 바로 뒤의 `ㅎㅏㄴㄱ` 낱자 확정도 백그라운드 키 전송에서 재현되는 도구 현상이다(아래 검증 방법 참고).
- `'%s\n' '한글 english 전환이 정상입니다.'`(`printf` 누락): **판정 불가.** `input-trace.txt`는 IME 경로만 기록하고 일반 ASCII 키(`key_down`)는 기록하지 않아, `printf`가 앱에 도달했는지 알 수 없다. 같은 줄을 다시 입력한 다음 줄은 정상이다. 이번 검증에서 ASCII 명령 약 40개(HID 키, 붙여넣기, cmux 입력)를 입력했을 때 누락은 재현되지 않았다.

### 검증 방법과 한계

- 키 입력은 시스템 HID 이벤트 탭에 `CGEvent`를 보내는 도구로 수행했다(`target/validation/run2/tools/`). 보내기 전에 대상 앱이 frontmost인지 확인한다. 사람의 실제 키보드 입력은 아니다. 키 반복은 autorepeat 필드를 설정한 이벤트로 확인했으며, 하드웨어로 키를 누르고 있는 상황은 아니다.
- cmux의 백그라운드 키 전송은 한국어 IME 조합을 거치지 않아 낱자로 확정되고, ABC 자판에서는 `type_text` 문자열이 모두 `a`로 들어갔다. 둘 다 도구의 한계이므로 판정에 사용하지 않았다.
- `TISSelectInputSource`로 입력 소스를 바꾸면 조합 중인 문자가 사라지고 다음 한국어 입력 때 다시 나타났다(`ime1.txt`, `sw-bf.txt`). 사용자 단축키(`Ctrl+Space`)로 전환하면 정상이었으므로(`sw2-bf.txt`) 영문 전환 판정에는 단축키 결과를 사용했다.
- 창 크기는 System Events 자동화 권한이 없어 창 모서리를 드래그해서 바꿨다.
- 외부 모니터, Retina 배율 변경, 최소화·복원은 실행하지 않았다.

## SSH fixture

`script/prepare-validation`은 `mamba` `claude` 환경과 프로젝트 루트의 `requirements.txt`(`paramiko==4.0.0`)로 fixture 의존성을 `.tools/ssh-fixture`에 설치한다.
14:17 실행 결과 종료 코드 0이었고 `Cargo.lock`은 바뀌지 않았다. fixture는 연결 1회를 처리한 뒤 `ssh/finished`를 기록하고 정상 종료했다.

## 종료 시 프로세스 정리

2026-10-26 13:54–13:56, `./script/bundle --validation`으로 만든 `target/bareframe-validation.app`(clean shell `zsh -f -i`, cwd `target/validation`)에서 수행했다.
경로마다 앱을 새로 실행하고, 앱의 터미널 안에서 `tests/lifecycle_probe.sh lifecycle`을 실행했다.
프로브는 HUP/TERM을 받으면 `child-exit`에 `hangup`을 쓰고, 300초 안에 받지 못하면 `timed-out`을 쓴다.

| 경로 | 프로브 실행 | 프로브 pgrp와 세션 | `child-exit` | 남은 프로세스 | 결과 |
| --- | --- | --- | --- | --- | --- |
| 빨간 닫기 버튼 | `&` 백그라운드 job | 셸과 다른 pgrp, 같은 세션·tty | `hangup` | 없음 | PASS |
| Cmd+W | `&` 백그라운드 job | 셸과 다른 pgrp, 같은 세션·tty | `hangup` | 없음 | PASS |
| Cmd+Q | `&` 백그라운드 job | 셸과 다른 pgrp, 같은 세션·tty | `hangup` | 없음 | PASS |
| 빨간 닫기 버튼 | foreground (TPGID = 프로브 pgrp) | 같은 세션·tty | `hangup` | 없음 | PASS |

pgrp와 세션은 `ps -o pid,ppid,pgid,tpgid,tt`와 `os.getsid()`로 확인했다. 프로세스 트리는 `bareframe` → `/usr/bin/login`(세션 리더) → `zsh` → 프로브이며, 확인 대상은 프로브·zsh·login·앱 프로세스다.
"남은 프로세스 없음"은 닫은 뒤 2초 안에 이 프로세스들이 `ps`에 없었음을 뜻한다.

이전 기록의 `timed-out`(12:59)은 프로브의 20초 제한이 창을 닫기 전에 끝나서 생긴 결과이며, SIGHUP 미전달의 근거가 아니다.
`nohup`·`disown`으로 분리한 프로세스는 이번 범위에 포함하지 않았다. `exit`과 사용자 zsh 설정에서의 `Cmd+Q`는 [GUI 수용 검증](#gui-수용-검증)에서 확인했다.

## 이어서 실행할 검증

1. Retina 배율 변경, 최소화·복원, 다른 배율 모니터 이동을 실행한다.
2. 사람이 실제 키보드로 한글 IME(마지막 자모 Backspace 포함)와 키 반복을 짧게 확인한다.
3. native 초기화 실패를 재현할 방법을 정하고 오류 처리를 확인한다.
