//! The file behind the editor: which files it opens, saving, reloading when the file
//! changes on disk, and the notices asking about unsaved or outside changes.
use super::{EditorEvent, EditorPane};
use crate::{
    watch::{Watch, watch_file},
    widgets,
};
use bareframe_core::text::Buffer;
use gpui::{Context, div, prelude::*, px};
use std::path::Path;

/// Larger files aren't opened in the editor at all.
const MAX_OPEN: u64 = 64 << 20;

pub(super) enum Notice {
    ConfirmClose,
    ChangedOnDisk,
    Error(String),
}

/// Whether bareframe's editor can open `path`: a regular file whose start reads as UTF-8
/// text without NUL bytes.
pub fn is_text_file(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    // Larger files go to the system's app: reading them here would stall the window.
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_OPEN)
    {
        return false;
    }
    let mut head = Vec::new();
    if file.take(8192).read_to_end(&mut head).is_err() || head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(&head) {
        Ok(_) => true,
        // Cut in the middle of a character at the 8 KB mark.
        Err(error) => error.error_len().is_none(),
    }
}

impl EditorPane {
    /// Whether the pane may close now; if not, the editor asks the user first.
    pub fn request_close(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.buffer.is_dirty() || self.discarded {
            return true;
        }
        self.to_rendered = false;
        self.notice = Some(Notice::ConfirmClose);
        cx.notify();
        false
    }

    /// `Rendered` for a Markdown file: straight away, or once unsaved changes are settled.
    pub(super) fn show_rendered(&mut self, cx: &mut Context<Self>) {
        if !crate::markdown::is_markdown(&self.path) {
            return;
        }
        if self.buffer.is_dirty() && !self.discarded {
            self.to_rendered = true;
            self.notice = Some(Notice::ConfirmClose);
            cx.notify();
        } else {
            cx.emit(EditorEvent::ShowRendered);
        }
    }

    /// After Save or Don't Save: on to what the question was asked for.
    pub(super) fn settled(&mut self, cx: &mut Context<Self>) {
        cx.emit(if std::mem::take(&mut self.to_rendered) {
            EditorEvent::ShowRendered
        } else {
            EditorEvent::Close
        });
    }

    pub(super) fn save(&mut self, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        let text = self.buffer.text();
        // Written in place, so the file keeps its permissions, owner and hard links.
        match std::fs::write(&self.path, &text) {
            Ok(()) => {
                self.disk = text;
                self.buffer.mark_saved();
                self.language_saved(cx);
                self.notice = None;
                cx.emit(EditorEvent::Changed);
                cx.notify();
                true
            }
            Err(error) => {
                self.notice = Some(Notice::Error(format!("Could not save: {error}")));
                cx.notify();
                false
            }
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        self.buffer = Buffer::new(&text);
        self.disk = text;
        self.anchor = self.buffer.clamp(self.anchor);
        self.head = self.buffer.clamp(self.head);
        self.marked = None;
        self.notice = None;
        self.text_changed(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn on_disk_change(&mut self, cx: &mut Context<Self>) {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        if text == self.disk {
            return;
        }
        if self.buffer.is_dirty() {
            self.notice = Some(Notice::ChangedOnDisk);
            cx.notify();
        } else {
            self.reload(cx);
        }
    }

    pub(super) fn watch(&self, cx: &mut Context<Self>) -> Option<Watch> {
        watch_file(&self.path, Self::on_disk_change, cx)
    }

    pub(super) fn render_notice(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let notice = self.notice.as_ref()?;
        let theme = self.theme;
        let name = crate::explorer::file_name(&self.path);
        let button = |id: &'static str, label: &'static str, primary: bool| {
            widgets::button(theme, id, label, primary)
                .px_2()
                .py(px(2.0))
        };
        let (message, buttons): (String, Vec<gpui::Stateful<gpui::Div>>) = match notice {
            Notice::ConfirmClose => (
                if self.to_rendered {
                    format!("Save changes to {name} before showing it rendered?")
                } else {
                    format!("Save changes to {name} before closing?")
                },
                vec![
                    button("editor-close-cancel", "Cancel", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            editor.notice = None;
                            editor.to_rendered = false;
                            cx.emit(EditorEvent::Kept);
                            cx.notify();
                        },
                    )),
                    button("editor-close-discard", "Don't Save", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            editor.discarded = true;
                            editor.notice = None;
                            editor.settled(cx);
                        },
                    )),
                    button("editor-close-save", "Save", true).on_click(cx.listener(
                        |editor, _, _, cx| {
                            if editor.save(cx) {
                                editor.settled(cx);
                            }
                        },
                    )),
                ],
            ),
            Notice::ChangedOnDisk => (
                format!("{name} changed on disk."),
                vec![
                    button("editor-keep", "Keep my changes", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            // Saving now would overwrite the outside change, as asked.
                            editor.disk = std::fs::read_to_string(&editor.path).unwrap_or_default();
                            editor.notice = None;
                            cx.notify();
                        },
                    )),
                    button("editor-reload", "Reload", true)
                        .on_click(cx.listener(|editor, _, _, cx| editor.reload(cx))),
                ],
            ),
            Notice::Error(error) => (
                error.clone(),
                vec![
                    button("editor-dismiss", "Dismiss", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            editor.notice = None;
                            cx.notify();
                        },
                    )),
                ],
            ),
        };
        Some(
            super::bar(theme)
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w_0().truncate().child(message))
                .children(buttons),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_are_told_from_binaries() {
        let dir = std::env::temp_dir().join(format!("bareframe-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = dir.join("a.rs");
        std::fs::write(&text, "fn main() {}\n// 한글\n").unwrap();
        let binary = dir.join("a.bin");
        std::fs::write(&binary, [0u8, 1, 2, 3]).unwrap();
        // A multi-byte character cut by the 8 KB read is still text.
        let cut = dir.join("cut.txt");
        let mut long = "a".repeat(8191);
        long.push('한');
        std::fs::write(&cut, long).unwrap();
        assert!(is_text_file(&text));
        assert!(!is_text_file(&binary));
        assert!(is_text_file(&cut));
        assert!(!is_text_file(&dir), "a directory is not a text file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
