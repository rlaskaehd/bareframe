//! Asks before a clipboard operation the terminal would not do on its own: an unsafe paste,
//! or a program reading or writing the clipboard (OSC 52) when Settings say to ask.
use super::WorkspaceView;
use crate::{theme::hsla, widgets};
use bareframe_core::{layout::PaneId, session::ClipboardOperation};
use gpui::{
    Context, FocusHandle, Subscription, Window, anchored, deferred, div, point, prelude::*, px,
};

/// Lines of the text shown in the prompt; enough to spot a hidden command.
const PREVIEW_LINES: usize = 12;
/// Characters shown per line; longer lines wrap up to this and say how much is left.
const PREVIEW_LINE_CHARS: usize = 400;
/// Clicks this soon after the prompt appears don't approve: they were aimed at what was
/// there before.
const CONFIRM_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

pub(super) struct ClipboardPrompt {
    pub(super) pane: PaneId,
    operation: ClipboardOperation,
    text: String,
    pub(super) focus: FocusHandle,
    /// Holds every key while the prompt is up; dropping the prompt releases them.
    _keys: Subscription,
    /// When the prompt was first drawn in the active window; the confirm delay runs from
    /// here and starts over whenever the window goes inactive.
    shown: std::cell::Cell<Option<std::time::Instant>>,
}

impl WorkspaceView {
    pub(super) fn show_clipboard_prompt(
        &mut self,
        pane: PaneId,
        operation: ClipboardOperation,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // One at a time: Ghostty holds a single request per terminal, and a second
        // terminal's request waits until this one is answered.
        if self.clipboard_prompt.is_some() {
            return self.answer(pane, false, cx);
        }
        // Nothing may cover the prompt, or a click aimed at the palette or a menu could
        // land on a button that was not visible.
        self.palette = None;
        self.menu = None;
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        // The prompt is modal. No shortcut runs under it: a new tab or split would put a
        // live terminal over it and take the keyboard. Esc refuses; no key approves, so a
        // stray Enter can't confirm a paste.
        let view = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            cx.stop_propagation();
            if event.keystroke.key == "escape" {
                view.update(cx, |view, cx| {
                    view.finish_clipboard_prompt(false, window, cx)
                })
                .ok();
            }
        });
        self.clipboard_prompt = Some(ClipboardPrompt {
            pane,
            operation,
            text,
            focus,
            _keys: keys,
            shown: std::cell::Cell::new(None),
        });
        // The prompt is drawn by GPUI, which native terminal surfaces would cover.
        if !self.frozen {
            self.set_frozen(true, cx);
        }
        cx.notify();
    }

    fn answer(&mut self, pane: PaneId, approved: bool, cx: &mut Context<Self>) {
        if let Some(terminal) = self.terminal(pane) {
            terminal.update(cx, |terminal, cx| terminal.resolve_clipboard(approved, cx));
        }
    }

    /// The pane asking has closed; its terminal already refused the request on the way out.
    pub(super) fn drop_clipboard_prompt_for(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self
            .clipboard_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.pane == pane)
        {
            self.clipboard_prompt = None;
            self.set_frozen(false, cx);
        }
    }

    fn finish_clipboard_prompt(
        &mut self,
        approved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(prompt) = self.clipboard_prompt.take() {
            self.answer(prompt.pane, approved, cx);
        }
        self.set_frozen(false, cx);
        self.sync(window, cx);
    }

    pub(super) fn render_clipboard_prompt(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let prompt = self.clipboard_prompt.as_ref()?;
        if !window.is_window_active() {
            prompt.shown.set(None);
        } else if prompt.shown.get().is_none() {
            prompt.shown.set(Some(std::time::Instant::now()));
        }
        let theme = self.theme();
        let (title, body, confirm) = match prompt.operation {
            ClipboardOperation::Paste => (
                "Warning: Potentially Unsafe Paste",
                "Pasting this text into the terminal may be dangerous as it looks like some commands may be executed.",
                "Paste",
            ),
            ClipboardOperation::Read => (
                "Allow Clipboard Read?",
                "A program in this terminal wants to read your clipboard, which holds:",
                "Allow",
            ),
            ClipboardOperation::Write => (
                "Allow Clipboard Write?",
                "A program in this terminal wants to replace your clipboard with:",
                "Allow",
            ),
        };
        // Who is asking, so a background tab can't pass off its request as the active one's.
        // The tab number is ours; the name and location come from the program, so they are
        // shown with nothing hidden.
        let tab = self
            .model
            .tabs
            .iter()
            .position(|tab| tab.layout.panes().contains(&prompt.pane));
        let source = tab.zip(self.pane(prompt.pane)).map(|(tab, pane)| {
            let name = visible(&pane.name(cx));
            match pane.location(cx) {
                Some(location) => format!("From tab {} ({name} · {})", tab + 1, visible(&location)),
                None => format!("From tab {} ({name})", tab + 1),
            }
        });
        // A lone `\r` also starts a line on screen, so it can't hide what follows it.
        let lines: Vec<&str> = prompt
            .text
            .lines()
            .flat_map(|line| line.split('\r'))
            .collect();
        let more = lines.len().saturating_sub(PREVIEW_LINES);
        let preview: Vec<String> = lines.into_iter().take(PREVIEW_LINES).map(visible).collect();
        let button = |id: &'static str, label: &'static str, primary: bool| {
            widgets::button(theme, id, label, primary)
                .px_3()
                .py(px(5.0))
        };
        let (width, height) = self.window_size();
        // Catches clicks outside the card, so the sidebar and top bar can't open a tab
        // or split under the prompt either.
        let backdrop = deferred(
            anchored()
                .position(point(px(0.0), px(0.0)))
                .child(div().occlude().w(px(width)).h(px(height))),
        )
        .with_priority(1);
        Some(
            div().child(backdrop).child(
                self.dialog(
                    520.0,
                    height * 0.18,
                    div()
                        .track_focus(&prompt.focus)
                        .p_4()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(div().text_color(hsla(theme.text_muted)).child(body))
                        .children(source.map(|source| {
                            div()
                                .text_size(px(12.0))
                                .text_color(hsla(theme.text_muted))
                                .truncate()
                                .child(source)
                        }))
                        .child(
                            div()
                                .p_2()
                                .rounded_md()
                                .bg(hsla(theme.terminal_background))
                                .font_family(self.mono_font())
                                .text_size(px(12.0))
                                .flex()
                                .flex_col()
                                .children(preview.into_iter().map(|line| div().child(line)))
                                .when(more > 0, |this| {
                                    this.child(
                                        div()
                                            .text_color(hsla(theme.text_muted))
                                            .child(format!("… {more} more lines")),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(button("clipboard-cancel", "Cancel", false).on_click(
                                    cx.listener(|view, _, window, cx| {
                                        view.finish_clipboard_prompt(false, window, cx)
                                    }),
                                ))
                                .child(button("clipboard-confirm", confirm, true).on_click(
                                    cx.listener(|view, _, window, cx| {
                                        let early =
                                            view.clipboard_prompt.as_ref().is_some_and(|prompt| {
                                                prompt.shown.get().is_none_or(|shown| {
                                                    shown.elapsed() < CONFIRM_DELAY
                                                })
                                            });
                                        if !early {
                                            view.finish_clipboard_prompt(true, window, cx)
                                        }
                                    }),
                                )),
                        ),
                ),
            ),
        )
    }
}

/// A preview line with nothing hidden: control characters as their Unicode pictures
/// (`␛`), invisible and direction-changing characters as `<U+202E>`, and a long line cut
/// with a note of how much is left.
fn visible(line: &str) -> String {
    let mut out = String::new();
    for (count, ch) in line.chars().enumerate() {
        if count == PREVIEW_LINE_CHARS {
            let rest = line.chars().count() - count;
            out.push_str(&format!(" … {rest} more characters"));
            break;
        }
        match ch {
            '\u{0}'..='\u{1f}' => out.extend(char::from_u32(0x2400 + ch as u32)),
            '\u{7f}' => out.push('\u{2421}'),
            '\u{80}'..='\u{9f}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2069}'
            | '\u{feff}' => out.push_str(&format!("<U+{:04X}>", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::visible;

    #[test]
    fn hidden_characters_are_shown() {
        assert_eq!(
            visible("a\u{1b}[2Jb\tc\u{7f}"),
            "a\u{241b}[2Jb\u{2409}c\u{2421}"
        );
        assert_eq!(visible("ls\u{202e}txt"), "ls<U+202E>txt");
        let long = "x".repeat(super::PREVIEW_LINE_CHARS + 5);
        assert!(visible(&long).ends_with(" … 5 more characters"));
    }
}
