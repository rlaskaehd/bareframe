# bareframe adapter changes

Upstream: https://github.com/behzade/gpui-libghostty
Revision: 2bbde4ae53a8e163cdbe05294bd09934b30c1af3
License: MIT, retained in LICENSE and vendored Ghostty LICENSE.

This independent copy is the application dependency. `reference/gpui-libghostty`
remains the unchanged upstream snapshot.

Local changes are limited to macOS embedding:

- Native preedit, cursor geometry and binding action entry points for GPUI IME.
- A bounded queue for title, working directory and command-finished observations.
- Explicit gap events if observations exceed the queue or text size limit.
- TERM_PROGRAM identifies bareframe.
- `send_key` gives Control chords their printable character as text, as Ghostty's
  AppKit view does. GPUI drops the text while Control is held, and without it
  Ghostty's encoder emitted nothing for chords such as ctrl+shift+a or ctrl+@
  instead of CSI u.
- `TerminalTheme` carries an optional `font_family` and `font_size`, written to
  the same generated config file as the colors, so the app's typography reaches
  every surface at spawn and through `TerminalThemeState::apply`. A family name
  containing a newline is dropped rather than allowed to start another key.
  The struct is no longer `Copy`/`Eq` because of these fields.

The application implements the GPUI view and text input handler in
`src/terminal/adapter.rs`. The upstream GPUI macro is retained for provenance but
is not used by bareframe. Linux is outside the MVP 1 target.

Command-finished observations do not identify commands or capture their output.
The MVP 2 shell protocol and output attribution still require their own design.
