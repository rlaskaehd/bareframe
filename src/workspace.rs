use crate::{
    app,
    explorer::Explorer,
    files,
    pane::{PaneEvent, TerminalPane},
    repo::Summary,
    review::CodeReview,
    search::SearchPanel,
    text_field::TextField,
    theme::{self, hsla},
};
use bareframe_core::{
    config::LaunchConfig,
    layout::{Axis, Direction, Layout, PaneId},
    settings::Settings,
    state::SavedState,
    workspace::{Closed, GroupId, TabId, Workspace},
};
use columns::Column;
pub use exit::Exit;
use gpui::{
    Bounds, Context, Entity, FocusHandle, MouseButton, MouseMoveEvent, Pixels, Subscription,
    Window, anchored, deferred, div, point, prelude::*, px,
};
use menu::ContextMenu;
use pane_view::PaneView;
use std::collections::HashMap;

mod browsers;
mod changes;
mod clipboard;
mod columns;
mod commands;
mod drag;
mod exit;
mod external;
mod layout;
mod menu;
mod palette;
mod pane_view;
mod persist;
mod prefs;
mod sidebar;
mod tools;
mod top_bar;
mod viewer;

#[derive(Clone, Copy, PartialEq)]
enum RenameTarget {
    Tab(TabId),
    Group(GroupId),
}

struct Resize {
    tab: usize,
    path: Vec<bool>,
    axis: Axis,
}

/// Owns tabs, layout and the panes; never a PTY or native surface directly.
pub struct WorkspaceView {
    model: Workspace,
    panes: HashMap<PaneId, (PaneView, Option<Subscription>)>,
    config: LaunchConfig,
    settings: Settings,
    /// `Cmd+0` returns here: the size from the config file, not a hard-coded one.
    base_font_size: f32,
    resize: Option<Resize>,
    /// Split containers of the active tab, recorded during prepaint for resizing.
    split_bounds: HashMap<Vec<bool>, Bounds<Pixels>>,
    should_move_window: bool,
    sidebar_open: bool,
    sidebar_width: f32,
    tools_open: bool,
    tools_width: f32,
    /// The tools panel's left edge, recorded on render for resizing.
    tools_left: f32,
    tool: tools::Tool,
    explorer: Entity<Explorer>,
    _explorer_events: Subscription,
    global_search: Entity<SearchPanel>,
    _search_events: Subscription,
    review: Entity<CodeReview>,
    _review_events: Subscription,
    review_open: bool,
    review_width: f32,
    /// The side column whose edge is being dragged.
    resizing_column: Option<Column>,
    /// The active pane's repository and the diff button's numbers.
    repo: Option<(std::path::PathBuf, Summary)>,
    /// The directory last looked up, so moving focus doesn't run git again.
    repo_dir: Option<std::path::PathBuf>,
    _repo_task: Option<gpui::Task<()>>,
    _repo_watch: Option<crate::watch::Watch>,
    /// Tabs being dragged in the sidebar; their rows show an empty slot meanwhile.
    dragging: Vec<TabId>,
    /// Where a dragged pane or tab would land: beside this pane, on that side.
    drop_target: Option<(PaneId, Direction)>,
    /// The terminals were frozen for a drag, so the drop zones can be drawn over them.
    drag_frozen: bool,
    /// The explorer's file being dragged, when it can be dropped beside a pane.
    dragged_file: Option<std::path::PathBuf>,
    menu: Option<ContextMenu>,
    clipboard_prompt: Option<clipboard::ClipboardPrompt>,
    /// A quit or window close waiting for answers about unsaved editor changes.
    pending_exit: Option<Exit>,
    palette: Option<palette::Palette>,
    /// The shown surfaces are swapped for snapshots while a pane menu covers them.
    frozen: bool,
    frozen_panes: Vec<PaneId>,
    renaming: Option<(RenameTarget, Entity<TextField>, Subscription)>,
    /// The sidebar's tab filter: the field plus its event and change subscriptions.
    search: Option<(Entity<TextField>, [Subscription; 2])>,
    /// Set only when it changes; render runs every frame.
    window_title: String,
    /// Recorded on render so the layout can be saved without a window at hand.
    window_bounds: Option<[f32; 4]>,
    /// The last text written to the state file.
    saved: String,
    focus: FocusHandle,
    _quit: Subscription,
    _autosave: gpui::Task<()>,
    /// The settings file's text as last read or written, to skip our own writes on reload.
    config_text: String,
    dark_appearance: bool,
    _appearance: Subscription,
    _updates: Subscription,
    _config_watch: Option<crate::watch::Watch>,
}

impl WorkspaceView {
    pub fn new(
        config: LaunchConfig,
        settings: Settings,
        config_text: String,
        saved: Option<SavedState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (model, first) = Workspace::new();
        let dark_appearance = prefs::is_dark(window.appearance());
        let theme = prefs::theme_for(&settings, dark_appearance);
        let explorer = cx.new(|cx| Explorer::new(theme, cx));
        let show_hidden = settings.flag("show-hidden-files");
        explorer.update(cx, |explorer, cx| explorer.set_show_hidden(show_hidden, cx));
        let explorer_events = cx.subscribe_in(&explorer, window, |view, _, event, window, cx| {
            view.on_explorer_event(event, window, cx)
        });
        let global_search = cx.new(|cx| SearchPanel::new(theme, window, cx));
        let search_events =
            cx.subscribe_in(&global_search, window, |view, _, event, window, cx| {
                view.on_search_event(event, window, cx)
            });
        let mono = settings.font_family().unwrap_or("Menlo").to_owned();
        let review = cx.new(|_| CodeReview::new(theme, mono));
        let review_events = cx.subscribe_in(&review, window, |view, _, event, window, cx| {
            view.on_review_event(event, window, cx)
        });
        let mut view = Self {
            model,
            panes: HashMap::new(),
            base_font_size: settings.font_size(),
            settings,
            config,
            resize: None,
            split_bounds: HashMap::new(),
            should_move_window: false,
            sidebar_open: true,
            sidebar_width: columns::SIDEBAR_DEFAULT_WIDTH,
            tools_open: false,
            tools_width: columns::TOOLS_DEFAULT_WIDTH,
            tools_left: 0.0,
            tool: tools::Tool::Explorer,
            explorer,
            _explorer_events: explorer_events,
            global_search,
            _search_events: search_events,
            review,
            _review_events: review_events,
            review_open: false,
            review_width: columns::REVIEW_DEFAULT_WIDTH,
            resizing_column: None,
            repo: None,
            repo_dir: None,
            _repo_task: None,
            _repo_watch: None,
            dragging: Vec::new(),
            drop_target: None,
            drag_frozen: false,
            dragged_file: None,
            menu: None,
            clipboard_prompt: None,
            pending_exit: None,
            palette: None,
            frozen: false,
            frozen_panes: Vec::new(),
            renaming: None,
            search: None,
            window_title: String::new(),
            window_bounds: None,
            saved: String::new(),
            focus: cx.focus_handle(),
            _quit: cx.on_app_quit(|view, cx| {
                view.shutdown(cx);
                async {}
            }),
            config_text,
            dark_appearance,
            _appearance: cx.observe_window_appearance(window, |view, window, cx| {
                view.dark_appearance = prefs::is_dark(window.appearance());
                view.apply_settings(cx);
            }),
            _updates: cx.observe(&crate::update::global(cx), |_, _, cx| cx.notify()),
            _config_watch: None,
            // ponytail: compares the encoded layout every 2s instead of tracking each
            // change; switch to change notifications if the layout ever gets large.
            _autosave: cx.spawn(async move |view, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(2))
                        .await;
                    if view.update(cx, |view, cx| view.save_state(cx)).is_err() {
                        break;
                    }
                }
            }),
        };
        // Before any terminal starts, so restored ones get the socket too.
        view.apply_browser_control(cx);
        crate::browser::control::register_workspace(
            cx.entity().downgrade(),
            window.window_handle(),
        );
        match saved {
            Some(saved) => view.restore(saved, window, cx),
            None => {
                let cwd = view.config.cwd.clone();
                view.open_pane(first, cwd, window, cx);
            }
        }
        view.watch_config(cx);
        view.apply_update_setting(cx);
        view.sync(window, cx);
        view
    }

    fn open_pane(
        &mut self,
        id: PaneId,
        cwd: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut config = LaunchConfig {
            cwd,
            ..self.config.clone()
        };
        // Settings › Default shell, if it names an existing file; else the launch shell.
        let shell = std::path::Path::new(self.settings.get("shell"));
        if shell.is_absolute() && shell.is_file() {
            config.shell = shell.to_path_buf();
        }
        let theme = self.terminal_theme();
        let pane = cx.new(|cx| TerminalPane::new(id, config, &theme, window, cx));
        let subscription =
            cx.subscribe_in(
                &pane,
                window,
                move |view, _, event, window, cx| match event {
                    PaneEvent::Exited => view.close_pane(id, window, cx),
                    PaneEvent::Clipboard(operation, text) => {
                        view.show_clipboard_prompt(id, *operation, text.clone(), window, cx)
                    }
                    PaneEvent::TitleChanged => {
                        view.refresh_explorer_roots(cx);
                        view.refresh_repo(false, cx);
                        cx.notify()
                    }
                    // A command may have committed, checked out or edited files.
                    PaneEvent::CommandFinished => view.refresh_repo(true, cx),
                },
            );
        self.panes
            .insert(id, (PaneView::Terminal(pane), Some(subscription)));
    }

    fn pane(&self, id: PaneId) -> Option<&PaneView> {
        self.panes.get(&id).map(|(pane, _)| pane)
    }

    fn terminal(&self, id: PaneId) -> Option<&Entity<TerminalPane>> {
        self.pane(id)?.terminal()
    }

    fn pane_title(&self, id: PaneId, cx: &Context<Self>) -> String {
        self.pane(id)
            .map_or_else(String::new, |pane| pane.title(cx))
    }

    /// The active pane's cwd comes from OSC 7, which any local program's output can send
    /// (Ghostty already rejects remote hosts), so only an existing directory is trusted.
    fn active_cwd(&self, cx: &Context<Self>) -> std::path::PathBuf {
        let dir = match self.pane(self.model.active_pane()) {
            Some(PaneView::Terminal(pane)) => pane.read(cx).cwd().canonicalize().ok(),
            // A file's pane works in the file's folder.
            Some(pane @ (PaneView::Editor(_) | PaneView::Markdown(_))) => {
                pane.file(cx).and_then(|path| path.parent().map(Into::into))
            }
            _ => None,
        };
        dir.filter(|cwd| cwd.is_dir())
            .unwrap_or_else(|| self.config.cwd.clone())
    }

    /// The active tab's panes on screen: only the active one while maximized.
    fn shown_panes(&self) -> Vec<PaneId> {
        let tab = self.model.active();
        if tab.maximized {
            vec![tab.active_pane]
        } else {
            tab.layout.panes()
        }
    }

    fn set_frozen(&mut self, frozen: bool, cx: &mut Context<Self>) {
        self.frozen = frozen;
        // Thaw exactly the panes that were frozen: the active tab may have changed since.
        let panes = if frozen {
            self.shown_panes()
        } else {
            std::mem::take(&mut self.frozen_panes)
        };
        for id in &panes {
            match self.pane(*id) {
                Some(PaneView::Terminal(pane)) => {
                    pane.update(cx, |pane, cx| pane.set_frozen(frozen, cx))
                }
                Some(PaneView::Browser(pane)) => {
                    pane.update(cx, |pane, cx| pane.set_frozen(frozen, cx))
                }
                _ => {}
            }
        }
        if frozen {
            self.frozen_panes = panes;
        }
    }

    /// Shows only the active tab's surfaces and gives the active pane keyboard focus.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.frozen {
            self.palette = None;
            self.dismiss_menu(cx);
        }
        let visible = self.shown_panes();
        // A clipboard prompt keeps the native views frozen under it until it is answered.
        if self.clipboard_prompt.is_none() {
            for (id, (pane, _)) in &self.panes {
                let shown = visible.contains(id);
                match pane {
                    PaneView::Terminal(pane) => {
                        pane.update(cx, |pane, cx| pane.set_visible(shown, cx))
                    }
                    PaneView::Browser(pane) => {
                        pane.update(cx, |pane, cx| pane.set_visible(shown, cx))
                    }
                    _ => {}
                }
            }
        }
        // A clipboard prompt keeps the keyboard until it is answered.
        if let Some(prompt) = &self.clipboard_prompt {
            prompt.focus.focus(window, cx);
        } else if let Some(pane) = self.pane(self.model.active_pane()).cloned() {
            pane.focus(window, cx);
        }
        self.split_bounds.clear();
        self.refresh_explorer_roots(cx);
        self.refresh_repo(false, cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = match self.settings.get("working-directory") {
            "home" => files::home_dir().unwrap_or_else(|| self.config.cwd.clone()),
            _ => self.active_cwd(cx),
        };
        let color = self.model.active().color;
        let pane = self.model.new_tab();
        if self.settings.flag("preserve-tab-color") {
            let index = self.model.active_tab;
            self.model.set_tab_color(index, color);
        }
        if self.settings.get("new-tab-position") == "end" {
            let (index, end) = (self.model.active_tab, self.model.tabs.len());
            let group = self.model.active().group;
            self.model.move_tabs(&[index], end, group);
        }
        self.open_pane(pane, cwd, window, cx);
        self.sync(window, cx);
    }

    fn new_tab_at(&mut self, cwd: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.model.new_tab();
        self.open_pane(pane, cwd, window, cx);
        self.sync(window, cx);
    }

    /// A new tab at `dir` running `command`, typed in so its output shows there.
    fn run_in_new_tab(
        &mut self,
        command: &str,
        dir: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab_at(dir, window, cx);
        if let Some(terminal) = self.terminal(self.model.active_pane()) {
            terminal.update(cx, |terminal, cx| {
                terminal.type_text(&format!("{command}\r"), cx)
            });
        }
    }

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.active_cwd(cx);
        let pane = self.model.split(axis);
        self.open_pane(pane, cwd, window, cx);
        self.sync(window, cx);
    }

    fn close_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        // An editor with unsaved changes asks first, and closes itself once answered.
        if let Some(PaneView::Editor(tabs)) = self.pane(id).cloned()
            && !tabs.update(cx, |tabs, cx| tabs.request_close(cx))
        {
            if self.model.focus(id) {
                self.sync(window, cx);
            }
            return;
        }
        self.drop_clipboard_prompt_for(id, cx);
        if let Some((PaneView::Terminal(pane), _)) = self.panes.remove(&id) {
            pane.update(cx, |pane, cx| pane.shutdown(cx));
        }
        match self.model.close(id) {
            Some(Closed::Replaced(fresh)) => {
                let cwd = self.config.cwd.clone();
                self.open_pane(fresh, cwd, window, cx);
                self.sync(window, cx);
            }
            Some(_) => self.sync(window, cx),
            None => {}
        }
    }

    fn close_tabs(&mut self, indices: &[usize], window: &mut Window, cx: &mut Context<Self>) {
        let panes: Vec<PaneId> = indices
            .iter()
            .filter_map(|&index| self.model.tabs.get(index))
            .flat_map(|tab| tab.layout.panes())
            .collect();
        for pane in panes {
            self.close_pane(pane, window, cx);
        }
    }

    fn new_tab_in_group(&mut self, group: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&last) = self.model.group_tabs(group).last() {
            self.model.select_tab(last);
            self.new_tab(window, cx);
        }
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.select_tab(index) {
            self.sync(window, cx);
        }
    }

    fn focus_direction(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.focus_direction(direction) {
            self.sync(window, cx);
        }
    }

    fn drag_divider(&mut self, event: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
        if self.drag_column_edge(event, window) {
            cx.notify();
            return;
        }
        let Some(resize) = &self.resize else {
            return;
        };
        if !event.dragging() || resize.tab != self.model.active_tab {
            self.resize = None;
            return;
        }
        let Some(bounds) = self.split_bounds.get(&resize.path) else {
            return;
        };
        let ratio = match resize.axis {
            Axis::Horizontal => (event.position.x - bounds.origin.x) / bounds.size.width,
            Axis::Vertical => (event.position.y - bounds.origin.y) / bounds.size.height,
        };
        let layout = &mut self.model.tabs[resize.tab].layout;
        if layout.set_ratio(&resize.path, ratio) {
            cx.notify();
        }
    }

    /// The window's size in points, as recorded on the last render.
    fn window_size(&self) -> (f32, f32) {
        self.window_bounds
            .map_or((1040.0, 720.0), |[_, _, width, height]| (width, height))
    }

    /// A dialog card centered across the window, `top` points down and at most `width`
    /// wide, drawn above menus. `card` brings its own content and padding.
    fn dialog(&self, width: f32, top: f32, card: gpui::Div) -> impl IntoElement {
        let theme = self.theme();
        let (window_width, _) = self.window_size();
        let width = width.min(window_width - 40.0);
        deferred(
            anchored()
                .position(point(px((window_width - width) / 2.0), px(top)))
                .child(
                    card.occlude()
                        .w(px(width))
                        .flex()
                        .flex_col()
                        .rounded_lg()
                        .border_1()
                        .border_color(hsla(theme.border))
                        .bg(hsla(theme.tab_active))
                        .shadow_lg()
                        .font_family(theme::UI_FONT)
                        .text_size(px(13.0))
                        .text_color(hsla(theme.text)),
                ),
        )
        .with_priority(2)
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let title = format!(
            "{} — {}",
            self.pane_title(self.model.active_pane(), cx),
            app::name()
        );
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        let bounds = window.bounds();
        self.tools_left = if self.sidebar_open {
            self.column_width(Column::Sidebar, window) + 1.0
        } else {
            0.0
        };
        self.window_bounds = Some([
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ]);
        let tab = self.model.active();
        let layout = if tab.maximized {
            Layout::Pane(tab.active_pane)
        } else {
            tab.layout.clone()
        };
        let content = self.render_layout(&layout, &mut Vec::new(), cx);
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.terminal_background))
            .text_color(hsla(theme.text))
            .on_mouse_move(
                cx.listener(|view, event, window, cx| view.drag_divider(event, window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                // Also ends a tab drag that was dropped outside any drop target.
                cx.listener(|view, _, _, cx| {
                    view.resize = None;
                    view.resizing_column = None;
                    if view.drag_frozen || view.drop_target.is_some() || !view.dragging.is_empty() {
                        view.end_drag(cx);
                    }
                }),
            )
            .map(|this| self.with_actions(this, cx))
            // The terminal keeps focus under a menu, so Esc is caught on the way down.
            // Opening a menu from the sidebar moves focus off the terminal; `sync` closes
            // the menu and gives focus back.
            .capture_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                if view.menu.is_some() && event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    view.sync(window, cx);
                }
            }))
            .when_some(self.menu.as_ref(), |this, menu| {
                this.child(self.render_menu(menu, cx))
            })
            .children(self.render_clipboard_prompt(window, cx))
            .children(self.render_palette(cx))
            .child(self.render_top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .when(self.sidebar_open, |this| {
                        let width = self.column_width(Column::Sidebar, window);
                        this.child(self.render_sidebar(width, cx))
                            .child(self.column_edge(Column::Sidebar, cx))
                    })
                    .when(self.tools_open, |this| {
                        let width = self.column_width(Column::Tools, window);
                        this.child(self.render_tools(width, cx))
                            .child(self.column_edge(Column::Tools, cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().child(content))
                    .when(self.review_open, |this| {
                        let width = self.column_width(Column::Review, window);
                        this.child(self.render_review(width, cx))
                    }),
            )
    }
}
