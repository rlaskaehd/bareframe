//! Files opened inside bareframe: Markdown in the viewer, other text in the code editor.
use super::{PaneView, WorkspaceView, external::open_externally};
use crate::{
    editor::{EditorPane, LineNumbers, is_text_file},
    editor_tabs::{EditorTabs, TabsEvent},
    markdown::{MarkdownEvent, MarkdownPane, Target, is_markdown},
};
use bareframe_core::layout::{Axis, PaneId};
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

    /// A file already open is shown again rather than opened twice. Otherwise it joins
    /// the active tab's editor as a file tab (Settings can turn that off), or gets a pane.
    fn open_editor(
        &mut self,
        path: PathBuf,
        line: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.canonicalize().unwrap_or(path);
        let open = self.panes.iter().find_map(|(id, (pane, _))| match pane {
            PaneView::Editor(tabs) => tabs
                .update(cx, |tabs, cx| tabs.show_path(&path, cx))
                .then_some(*id),
            _ => None,
        });
        let id = match open.or_else(|| self.group_for(&path, cx)) {
            Some(id) => {
                if open.is_none()
                    && let Some(PaneView::Editor(tabs)) = self.pane(id).cloned()
                {
                    let editor = self.new_editor(path, cx);
                    tabs.update(cx, |tabs, cx| tabs.add(editor, cx));
                }
                self.model.focus(id);
                id
            }
            None => {
                let id = self.new_file_pane();
                self.insert_editor_pane(id, vec![path], 0, window, cx);
                id
            }
        };
        if let (Some(line), Some(PaneView::Editor(tabs))) = (line, self.pane(id))
            && let Some(editor) = tabs.read(cx).active().cloned()
        {
            editor.update(cx, |editor, cx| editor.go_to_line(line, cx));
        }
        self.sync(window, cx);
    }

    /// The active tab's editor pane a new file joins, when grouping is on. Markdown keeps a
    /// pane of its own, since its rendered view replaces the whole pane.
    fn group_for(&self, path: &Path, cx: &Context<Self>) -> Option<PaneId> {
        if !self.settings.flag("group-editor-files") || is_markdown(path) {
            return None;
        }
        self.model.active().layout.panes().into_iter().find(|id| {
            matches!(self.pane(*id), Some(PaneView::Editor(tabs))
                if tabs.read(cx).editors().all(|editor| !is_markdown(editor.read(cx).path())))
        })
    }

    fn new_editor(&self, path: PathBuf, cx: &mut Context<Self>) -> gpui::Entity<EditorPane> {
        let theme = self.theme();
        let (mono, size) = (self.mono_font(), self.settings.font_size());
        let editor = cx.new(|cx| EditorPane::new(path, theme, mono, size, cx));
        self.apply_editor_options(&editor, cx);
        editor
    }

    /// Beside the active pane or in a new tab, per Settings.
    fn new_file_pane(&mut self) -> PaneId {
        match self.settings.get("open-file-layout") {
            "tab" => self.model.new_tab(),
            _ => self.model.split(Axis::Horizontal),
        }
    }

    /// An editor pane with `paths` as its file tabs, `active` shown.
    pub(super) fn insert_editor_pane(
        &mut self,
        id: PaneId,
        paths: Vec<PathBuf>,
        active: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors: Vec<_> = paths
            .into_iter()
            .map(|path| self.new_editor(path, cx))
            .collect();
        let theme = self.theme();
        let tabs = cx.new(|cx| {
            let mut tabs = EditorTabs::new(theme);
            for editor in editors {
                tabs.add(editor, cx);
            }
            tabs.show_index(active);
            tabs
        });
        let events = cx.subscribe_in(
            &tabs,
            window,
            move |view, _, event, window, cx| match event {
                TabsEvent::Changed => cx.notify(),
                TabsEvent::Close => {
                    view.close_pane(id, window, cx);
                    view.resume_exit(window, cx);
                }
                TabsEvent::Kept => view.pending_exit = None,
                TabsEvent::ShowRendered => view.swap_view(id, false, window, cx),
                TabsEvent::OpenAt(path, line) => view.open_file_at(path, Some(*line), window, cx),
                TabsEvent::RunInTerminal(command, dir) => {
                    view.run_in_new_tab(command, dir.clone(), window, cx)
                }
            },
        );
        self.panes
            .insert(id, (PaneView::Editor(tabs), Some(events)));
    }

    /// A Markdown file's two views trade places in the same pane.
    fn swap_view(&mut self, id: PaneId, raw: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = match self.pane(id) {
            Some(PaneView::Markdown(pane)) if raw => pane.read(cx).path().to_path_buf(),
            Some(PaneView::Editor(tabs)) if !raw => {
                let editor = tabs.read(cx).active().cloned();
                editor
                    .map(|editor| editor.read(cx).path().to_path_buf())
                    .unwrap_or_default()
            }
            _ => return,
        };
        self.panes.remove(&id);
        if raw {
            self.insert_editor_pane(id, vec![path], 0, window, cx);
        } else {
            self.insert_markdown_pane(id, path, window, cx);
        }
        self.model.focus(id);
        self.sync(window, cx);
    }

    pub(super) fn apply_editor_options(
        &self,
        pane: &gpui::Entity<EditorPane>,
        cx: &mut Context<Self>,
    ) {
        let numbers = LineNumbers::from_setting(self.settings.get("editor-line-numbers"));
        let autosave = self.settings.flag("editor-autosave");
        pane.update(cx, |pane, cx| pane.set_options(numbers, autosave, cx));
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
                // Through the same door as the explorer: text opens in the editor, and only
                // plain documents ever reach the system's apps.
                MarkdownEvent::Open(Target::File(path)) => view.open_file(path, window, cx),
                MarkdownEvent::Insert(text) => view.insert_into_terminal(id, text, window, cx),
                MarkdownEvent::ShowRaw => view.swap_view(id, true, window, cx),
            },
        );
        self.panes
            .insert(id, (PaneView::Markdown(pane), Some(events)));
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
