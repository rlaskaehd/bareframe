//! Warp's "Group files into single editor pane": an editor pane holds several files as
//! tabs. One file shows no tab strip; the pane header already names it.
use crate::{
    editor::{EditorEvent, EditorPane},
    theme::{self, Theme, hsla},
};
use gpui::{Context, Entity, EventEmitter, Subscription, Window, div, prelude::*, px};
use std::path::Path;

pub enum TabsEvent {
    /// A file's text or saved state changed, or the shown file did.
    Changed,
    /// Every file is closed (or settled for closing); the pane may go.
    Close,
    /// The Markdown file asked for its rendered view.
    ShowRendered,
}

struct File {
    editor: Entity<EditorPane>,
    _events: Subscription,
}

pub struct EditorTabs {
    files: Vec<File>,
    active: usize,
    /// Closing the whole pane is under way: each file answers in turn.
    closing: bool,
    theme: &'static Theme,
}

impl EventEmitter<TabsEvent> for EditorTabs {}

impl EditorTabs {
    pub fn new(theme: &'static Theme) -> Self {
        Self {
            files: Vec::new(),
            active: 0,
            closing: false,
            theme,
        }
    }

    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn editors(&self) -> impl Iterator<Item = &Entity<EditorPane>> {
        self.files.iter().map(|file| &file.editor)
    }

    pub fn active(&self) -> Option<&Entity<EditorPane>> {
        self.files.get(self.active).map(|file| &file.editor)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Adds a file as the shown tab, after the current one.
    pub fn add(&mut self, editor: Entity<EditorPane>, cx: &mut Context<Self>) {
        let events = cx.subscribe(&editor, |tabs, editor, event: &EditorEvent, cx| {
            tabs.on_file_event(&editor, event, cx)
        });
        let at = (self.active + 1).min(self.files.len());
        self.files.insert(
            at,
            File {
                editor,
                _events: events,
            },
        );
        self.active = at;
        cx.emit(TabsEvent::Changed);
        cx.notify();
    }

    /// Shows the tab holding `path`, if any.
    pub fn show_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let Some(index) = self
            .files
            .iter()
            .position(|file| file.editor.read(cx).path() == path)
        else {
            return false;
        };
        self.show(index, cx);
        true
    }

    pub fn show_index(&mut self, index: usize) {
        self.active = index.min(self.files.len().saturating_sub(1));
    }

    fn show(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active = index;
        cx.emit(TabsEvent::Changed);
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active() {
            editor.update(cx, |editor, cx| editor.focus(window, cx));
        }
    }

    /// Whether every file may close now. The first with unsaved changes is shown and asks;
    /// its answer continues with the rest.
    pub fn request_close(&mut self, cx: &mut Context<Self>) -> bool {
        for index in 0..self.files.len() {
            let editor = self.files[index].editor.clone();
            if !editor.update(cx, |editor, cx| editor.request_close(cx)) {
                self.closing = true;
                self.show(index, cx);
                return false;
            }
        }
        self.closing = false;
        true
    }

    /// Cmd+W with several files: only the shown one goes.
    pub fn close_active(&mut self, cx: &mut Context<Self>) {
        self.close_file(self.active, cx);
    }

    fn close_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(editor) = self.files.get(index).map(|file| file.editor.clone()) else {
            return;
        };
        self.closing = false;
        if editor.update(cx, |editor, cx| editor.request_close(cx)) {
            self.remove(index, cx);
        } else {
            self.show(index, cx);
        }
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        self.files.remove(index);
        if self.files.is_empty() {
            return cx.emit(TabsEvent::Close);
        }
        if self.active >= index && self.active > 0 {
            self.active -= 1;
        }
        cx.emit(TabsEvent::Changed);
        cx.notify();
    }

    fn on_file_event(
        &mut self,
        editor: &Entity<EditorPane>,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::Changed => {
                cx.emit(TabsEvent::Changed);
                cx.notify();
            }
            EditorEvent::ShowRendered => cx.emit(TabsEvent::ShowRendered),
            // The file's question was answered: it goes, and a pane being closed goes on.
            EditorEvent::Close => {
                let Some(index) = self.files.iter().position(|file| &file.editor == editor) else {
                    return;
                };
                if self.closing {
                    if self.request_close(cx) {
                        cx.emit(TabsEvent::Close);
                    }
                } else {
                    self.remove(index, cx);
                }
            }
        }
    }

    fn render_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .flex()
            .flex_none()
            .h(px(30.0))
            .border_b_1()
            .border_color(hsla(theme.border))
            .bg(hsla(theme.chrome))
            .font_family(theme::UI_FONT)
            .text_size(px(12.0))
            .children(self.files.iter().enumerate().map(|(index, file)| {
                let chosen = index == self.active;
                div()
                    .id(("editor-tab", index))
                    .group("editor-tab")
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_3()
                    .pr_1()
                    .max_w(px(220.0))
                    .border_r_1()
                    .border_color(hsla(theme.border))
                    .when(chosen, |this| {
                        this.bg(hsla(theme.terminal_background))
                            .text_color(hsla(theme.text))
                    })
                    .when(!chosen, |this| {
                        this.text_color(hsla(theme.text_muted))
                            .hover(|style| style.bg(hsla(theme.tab_hover)))
                    })
                    .on_click(cx.listener(move |tabs, _, window, cx| {
                        tabs.show(index, cx);
                        tabs.focus(window, cx);
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(file.editor.read(cx).file_name()),
                    )
                    .child(
                        crate::workspace::icon_button(("editor-tab-close", index), "×", theme)
                            .on_click(cx.listener(move |tabs, _, _, cx| {
                                cx.stop_propagation();
                                tabs.close_file(index, cx);
                            })),
                    )
            }))
    }
}

impl Render for EditorTabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .when(self.files.len() > 1, |this| {
                this.child(self.render_strip(cx))
            })
            .children(
                self.active()
                    .cloned()
                    .map(|editor| div().flex_1().min_h_0().child(editor)),
            )
    }
}
