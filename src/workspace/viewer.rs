//! Files opened inside bareframe: Markdown in the viewer, other text in the code editor.
use super::{PaneView, WorkspaceView, tools::open_externally};
use crate::{
    editor::{EditorEvent, EditorPane, is_text_file},
    markdown::{MarkdownEvent, MarkdownPane, Target, is_markdown},
};
use bareframe_core::workspace::{Axis, PaneId};
use gpui::{AppContext, Context, Window};
use std::path::{Path, PathBuf};

impl WorkspaceView {
    /// Markdown opens in the viewer and other text in the editor when Settings say so;
    /// anything else goes to the system.
    pub(super) fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.open_file_at(path, None, window, cx)
    }

    /// As `open_file`; a text file opens with the cursor on `line` (1-based).
    pub(super) fn open_file_at(
        &mut self,
        path: &Path,
        line: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_markdown(path) && self.settings.flag("open-markdown-in-viewer") {
            self.open_markdown(path.to_path_buf(), window, cx);
        } else if self.settings.flag("open-files-in-editor") && is_text_file(path) {
            self.open_editor(path.to_path_buf(), line, window, cx);
        } else {
            open_externally(path);
        }
    }

    /// A file already open is shown again rather than opened twice.
    fn open_editor(
        &mut self,
        path: PathBuf,
        line: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.canonicalize().unwrap_or(path);
        let open = self.panes.iter().find_map(|(id, (pane, _))| match pane {
            PaneView::Editor(pane) if pane.read(cx).path() == path => Some(*id),
            _ => None,
        });
        let id = match open {
            Some(id) => {
                self.model.focus(id);
                id
            }
            None => {
                let id = self.new_file_pane();
                self.insert_editor_pane(id, path, window, cx);
                id
            }
        };
        if let (Some(line), Some(PaneView::Editor(editor))) = (line, self.pane(id)) {
            editor.update(cx, |editor, cx| editor.go_to_line(line, cx));
        }
        self.sync(window, cx);
    }

    /// Beside the active pane or in a new tab, per Settings.
    fn new_file_pane(&mut self) -> PaneId {
        match self.settings.get("open-file-layout") {
            "tab" => self.model.new_tab(),
            _ => self.model.split(Axis::Horizontal),
        }
    }

    pub(super) fn insert_editor_pane(
        &mut self,
        id: PaneId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.theme();
        let (mono, size) = (self.mono_font(), self.settings.font_size());
        let pane = cx.new(|cx| EditorPane::new(path, theme, mono, size, cx));
        let events = cx.subscribe_in(
            &pane,
            window,
            move |view, _, event, window, cx| match event {
                EditorEvent::Changed => cx.notify(),
                EditorEvent::Close => view.close_pane(id, window, cx),
            },
        );
        self.panes
            .insert(id, (PaneView::Editor(pane), Some(events)));
    }

    /// Before quitting or closing the window: the first editor with unsaved changes asks,
    /// and the quit waits for the answer. Returns whether nothing is left unsaved.
    pub fn confirm_unsaved(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let editors: Vec<(PaneId, gpui::Entity<EditorPane>)> = self
            .panes
            .iter()
            .filter_map(|(id, (pane, _))| match pane {
                PaneView::Editor(editor) => Some((*id, editor.clone())),
                _ => None,
            })
            .collect();
        for (id, editor) in editors {
            if !editor.update(cx, |editor, cx| editor.request_close(cx)) {
                self.model.focus(id);
                self.sync(window, cx);
                return false;
            }
        }
        true
    }

    /// A file already open is shown again rather than opened twice. Otherwise it opens
    /// beside the active pane or in a new tab, per Settings.
    pub(super) fn open_markdown(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.canonicalize().unwrap_or(path);
        let open = self.panes.iter().find_map(|(id, (pane, _))| match pane {
            PaneView::Markdown(pane) if pane.read(cx).path() == path => Some(*id),
            _ => None,
        });
        if let Some(id) = open {
            self.model.focus(id);
            return self.sync(window, cx);
        }
        let id = self.new_file_pane();
        self.insert_markdown_pane(id, path, window, cx);
        self.sync(window, cx);
    }

    pub(super) fn insert_markdown_pane(
        &mut self,
        id: PaneId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.theme();
        let mono = self.mono_font();
        let pane = cx.new(|cx| MarkdownPane::new(path, theme, mono, cx));
        let events = cx.subscribe_in(
            &pane,
            window,
            move |view, _, event, window, cx| match event {
                MarkdownEvent::Open(Target::Web(url)) => cx.open_url(url),
                MarkdownEvent::Open(Target::Markdown(path)) => {
                    view.open_markdown(path.clone(), window, cx)
                }
                MarkdownEvent::Open(Target::File(path)) => open_externally(path),
                MarkdownEvent::Insert(text) => view.insert_into_terminal(id, text, window, cx),
            },
        );
        self.panes
            .insert(id, (PaneView::Markdown(pane), Some(events)));
    }

    pub(super) fn mono_font(&self) -> String {
        self.settings.font_family().unwrap_or("Menlo").to_owned()
    }

    /// Warp's "insert into terminal": the text goes to a terminal in the viewer's tab
    /// through the clipboard and Ghostty's paste, so bracketed paste and the unsafe-paste
    /// prompt apply and nothing runs until Enter. It stays on the clipboard.
    fn insert_into_terminal(
        &mut self,
        source: PaneId,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.model.tab_of(source) else {
            return;
        };
        let Some(target) = self.model.tabs[tab]
            .layout
            .panes()
            .into_iter()
            .find(|pane| self.terminal(*pane).is_some())
        else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_owned()));
        self.model.focus(target);
        self.sync(window, cx);
        if let Some(terminal) = self.terminal(target) {
            terminal.update(cx, |terminal, cx| terminal.paste(cx));
        }
    }
}
