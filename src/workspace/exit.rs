//! Quitting and closing the window: editors with unsaved changes ask first, and the layout
//! is saved before the panes go.
use super::{PaneView, WorkspaceView};
use crate::editor_tabs::EditorTabs;
use bareframe_core::layout::PaneId;
use gpui::{Context, Window};

/// What waits on answers about unsaved changes.
#[derive(Clone, Copy)]
pub enum Exit {
    Quit,
    CloseWindow,
}

impl WorkspaceView {
    /// Quits or closes the window, unless an editor has unsaved changes: then it asks, and
    /// the exit goes on once every such editor is answered (Cancel stops it).
    pub fn exit(&mut self, exit: Exit, window: &mut Window, cx: &mut Context<Self>) {
        if !self.confirm_unsaved(window, cx) {
            self.pending_exit = Some(exit);
            return;
        }
        match exit {
            Exit::Quit => cx.quit(),
            Exit::CloseWindow => {
                self.shutdown(cx);
                window.remove_window();
            }
        }
    }

    /// The window's close button found unsaved changes; close once they are answered.
    pub fn wait_to_close(&mut self) {
        self.pending_exit = Some(Exit::CloseWindow);
    }

    pub(super) fn resume_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(exit) = self.pending_exit.take() {
            self.exit(exit, window, cx);
        }
    }

    /// Before quitting or closing the window: the first editor with unsaved changes asks,
    /// and the quit waits for the answer. Returns whether nothing is left unsaved.
    pub fn confirm_unsaved(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let groups: Vec<(PaneId, gpui::Entity<EditorTabs>)> = self
            .panes
            .iter()
            .filter_map(|(id, (pane, _))| match pane {
                PaneView::Editor(tabs) => Some((*id, tabs.clone())),
                _ => None,
            })
            .collect();
        for (id, tabs) in groups {
            if !tabs.update(cx, |tabs, cx| tabs.request_close(cx)) {
                self.model.focus(id);
                self.sync(window, cx);
                return false;
            }
        }
        true
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        // Quitting and closing the window both land here, before the panes go away.
        if !self.panes.is_empty() {
            self.save_state(cx);
        }
        for (_, (pane, _)) in self.panes.drain() {
            match pane {
                PaneView::Terminal(pane) => pane.update(cx, |pane, cx| pane.shutdown(cx)),
                PaneView::Browser(pane) => pane.update(cx, |pane, _| pane.close()),
                _ => {}
            }
        }
    }
}
