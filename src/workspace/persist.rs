//! Saving the window's layout to the state file and reopening it on the next launch.
use super::{PaneView, WorkspaceView};
use crate::files;
use bareframe_core::{
    state::{self, SavedPane, SavedState},
    workspace::PaneId,
};
use gpui::{App, Context, Window};
use std::collections::HashMap;

impl WorkspaceView {
    /// Reopens a saved layout: terminals start fresh shells in their last directory (home
    /// if it is gone); programs that were running are not restarted.
    pub(super) fn restore(
        &mut self,
        saved: SavedState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model = saved.workspace;
        self.sidebar_open = saved.sidebar_open;
        self.sidebar_width = saved.sidebar_width;
        self.tools_width = saved.tools_width;
        self.tools_open = saved.tools_open;
        self.update_tools(cx);
        self.review_open = saved.review_open;
        self.review_width = saved.review_width;
        let kinds: HashMap<PaneId, SavedPane> = saved.panes.into_iter().collect();
        let panes: Vec<PaneId> = self.model.panes().collect();
        for id in panes {
            match kinds.get(&id) {
                Some(SavedPane::Settings) => self.insert_settings_pane(id, window, cx),
                Some(SavedPane::Markdown { path }) if path.is_file() => {
                    self.insert_markdown_pane(id, path.clone(), window, cx)
                }
                Some(SavedPane::Editor { paths, active })
                    if paths.iter().any(|path| path.is_file()) =>
                {
                    let files: Vec<_> = paths
                        .iter()
                        .filter(|path| path.is_file())
                        .cloned()
                        .collect();
                    // Found by path: a deleted file before it would shift the index.
                    let shown = paths
                        .get(*active)
                        .and_then(|path| files.iter().position(|file| file == path))
                        .unwrap_or(0);
                    self.insert_editor_pane(id, files, shown, window, cx)
                }
                kind => {
                    let cwd = match kind {
                        Some(SavedPane::Terminal { cwd }) if cwd.is_dir() => cwd.clone(),
                        _ => self.config.cwd.clone(),
                    };
                    self.open_pane(id, cwd, window, cx);
                }
            }
        }
    }

    fn state(&self, cx: &App) -> SavedState {
        let panes = self
            .panes
            .iter()
            .map(|(id, (pane, _))| {
                let saved = match pane {
                    PaneView::Terminal(pane) => SavedPane::Terminal {
                        cwd: pane.read(cx).cwd().to_path_buf(),
                    },
                    PaneView::Settings(_) => SavedPane::Settings,
                    PaneView::Markdown(pane) => SavedPane::Markdown {
                        path: pane.read(cx).path().to_path_buf(),
                    },
                    PaneView::Editor(tabs) => {
                        let tabs = tabs.read(cx);
                        SavedPane::Editor {
                            paths: tabs
                                .editors()
                                .map(|editor| editor.read(cx).path().to_path_buf())
                                .collect(),
                            active: tabs.active_index(),
                        }
                    }
                };
                (*id, saved)
            })
            .collect();
        SavedState {
            workspace: self.model.clone(),
            panes,
            sidebar_open: self.sidebar_open,
            sidebar_width: self.sidebar_width,
            tools_open: self.tools_open,
            tools_width: self.tools_width,
            review_open: self.review_open,
            review_width: self.review_width,
            window: self.window_bounds,
        }
    }

    /// Writes the layout if it changed.
    pub(super) fn save_state(&mut self, cx: &App) {
        let mut state = self.state(cx);
        state.panes.sort_by_key(|(id, _)| *id);
        let text = state::encode(&state);
        if text == self.saved {
            return;
        }
        let Some(path) = files::state_file() else {
            return;
        };
        match files::write_atomically(&path, &text) {
            Ok(()) => self.saved = text,
            Err(error) => eprintln!("bareframe: saving {}: {error}", path.display()),
        }
    }
}
