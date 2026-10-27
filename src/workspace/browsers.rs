//! Browser panes in the workspace: opening one, and the tabs its pages open.
use super::{PaneView, WorkspaceView};
use crate::{
    browser::{self, BrowserPane, BrowserPaneEvent},
    files,
};
use bareframe_core::layout::PaneId;
use gpui::{AppContext, Context, Window};

impl WorkspaceView {
    /// File › New Browser Tab: a blank page waiting for an address.
    pub(super) fn new_browser_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Chromium can't start inside a run loop pass, as a menu's action runs; it is tried
        // again once the menu is gone.
        if browser::engine::needs_start() && browser::in_run_loop() {
            let (view, mut cx) = (cx.entity().downgrade(), window.to_async(cx));
            let retried = browser::between_events(move || {
                view.update_in(&mut cx, |view, window, cx| view.new_browser_tab(window, cx))
                    .ok();
            });
            if let Err(error) = retried {
                eprintln!("bareframe: browser: {error}");
            }
            return;
        }
        let id = self.model.new_tab();
        self.insert_browser_pane(id, String::new(), window, cx);
        self.sync(window, cx);
    }

    pub(super) fn insert_browser_pane(
        &mut self,
        id: PaneId,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Chromium's own files, beside the config; pages keep nothing on disk (see engine).
        let data = files::config_file()
            .map(|config| config.with_file_name("browser"))
            .unwrap_or_else(|| std::env::temp_dir().join("bareframe-browser"));
        let theme = self.theme();
        let pane = cx.new(|cx| BrowserPane::new(url, &data, theme, window, cx));
        // The page's title shows in the pane header and the sidebar.
        let events = cx.subscribe_in(
            &pane,
            window,
            move |view, _, event, window, cx| match event {
                BrowserPaneEvent::Changed => cx.notify(),
                BrowserPaneEvent::Focused => {
                    if view.model.active_pane() != id && view.model.focus(id) {
                        view.sync(window, cx);
                    }
                }
                BrowserPaneEvent::OpenTab(url) => {
                    let id = view.model.new_tab();
                    view.insert_browser_pane(id, url.clone(), window, cx);
                    view.sync(window, cx);
                }
            },
        );
        self.panes
            .insert(id, (PaneView::Browser(pane), Some(events)));
    }
}
