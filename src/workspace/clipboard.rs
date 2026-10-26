//! Asks before a clipboard operation the terminal would not do on its own: an unsafe paste,
//! or a program reading or writing the clipboard (OSC 52) when Settings say to ask.
use super::WorkspaceView;
use crate::theme::{self, hsla};
use bareframe_core::{session::ClipboardOperation, workspace::PaneId};
use gpui::{
    Context, FocusHandle, KeyDownEvent, Window, anchored, deferred, div, point, prelude::*, px,
};

/// Lines of the text shown in the prompt; enough to spot a hidden command.
const PREVIEW_LINES: usize = 12;

pub(super) struct ClipboardPrompt {
    pane: PaneId,
    operation: ClipboardOperation,
    text: String,
    focus: FocusHandle,
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
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.clipboard_prompt = Some(ClipboardPrompt {
            pane,
            operation,
            text,
            focus,
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
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let prompt = self.clipboard_prompt.as_ref()?;
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
        let mut preview: Vec<&str> = prompt.text.lines().take(PREVIEW_LINES).collect();
        let more = prompt.text.lines().count().saturating_sub(PREVIEW_LINES);
        if preview.is_empty() {
            preview.push("");
        }
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py(px(5.0))
                .rounded_md()
                .border_1()
                .border_color(hsla(if primary { theme.accent } else { theme.border }))
                .when(primary, |this| {
                    this.bg(hsla(theme.accent)).text_color(hsla(theme.chrome))
                })
                .hover(|style| style.opacity(0.85))
                .child(label)
        };
        let viewport = self
            .window_bounds
            .map_or((1040.0, 720.0), |[_, _, w, h]| (w, h));
        let width = 520.0_f32.min(viewport.0 - 40.0);
        Some(
            deferred(
                anchored()
                    .position(point(px((viewport.0 - width) / 2.0), px(viewport.1 * 0.18)))
                    .child(
                        div()
                            .track_focus(&prompt.focus)
                            .occlude()
                            .w(px(width))
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(hsla(theme.border))
                            .bg(hsla(theme.tab_active))
                            .shadow_lg()
                            .font_family(theme::UI_FONT)
                            .text_size(px(13.0))
                            .text_color(hsla(theme.text))
                            // Esc refuses; there is no key that approves, so a stray
                            // Enter can't confirm a paste.
                            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key == "escape" {
                                    view.finish_clipboard_prompt(false, window, cx);
                                }
                            }))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(div().text_color(hsla(theme.text_muted)).child(body))
                            .child(
                                div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(hsla(theme.terminal_background))
                                    .font_family(
                                        self.settings.font_family().unwrap_or("Menlo").to_owned(),
                                    )
                                    .text_size(px(12.0))
                                    .flex()
                                    .flex_col()
                                    .children(
                                        preview
                                            .into_iter()
                                            .map(|line| div().truncate().child(line.to_owned())),
                                    )
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
                                            view.finish_clipboard_prompt(true, window, cx)
                                        }),
                                    )),
                            ),
                    ),
            )
            .with_priority(2),
        )
    }
}
