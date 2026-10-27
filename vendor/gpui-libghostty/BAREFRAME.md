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
- `TerminalTheme::extra` carries further Ghostty `key = value` lines (cursor, padding,
  clipboard and other settings from the app's Settings screen), written after the font.
  A key or value with a control character, or a key containing `=`, is dropped.
- Hidden surfaces give back their memory. Ghostty's renderer kept its three
  screen-sized frame targets (IOSurfaces, about 15 MB each on a Retina window) for
  every surface, shown or not. In `src/renderer/generic.zig`, `setVisible(false)` now
  waits for frames in flight and shrinks each frame's target to 1×1 (the layer keeps
  showing the last presented one), and `drawFrame` does not draw while hidden, so a
  background tab's output can't size them again; showing the surface draws the current
  state. The shim starts each surface occluded, matching its hidden view, so a tab
  that is never shown never draws. Ten terminal tabs: 740 MB → 355 MB after visiting
  each, 565 MB → 255 MB right after restoring.

The application implements the GPUI view and text input handler in
`src/terminal/adapter.rs`. The upstream GPUI macro is retained for provenance but
is not used by bareframe. Linux is outside the MVP 1 target.

Command-finished observations do not identify commands or capture their output.
The MVP 2 shell protocol and output attribution still require their own design.
