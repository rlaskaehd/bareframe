use crate::{
    app::{self, *},
    explorer::Explorer,
    pane::{PaneEvent, TerminalPane},
    rename::RenameInput,
    search::SearchPanel,
    settings_pane::SettingsPane,
    theme::{Theme, hsla},
};
use bareframe_core::{
    config::LaunchConfig,
    settings::{Settings, ThemeName},
    state::{self, SavedPane, SavedState},
    workspace::{Axis, Closed, Direction, GroupId, Layout, PaneId, TabId, Workspace},
};
use gpui::{
    AnyView, App, Bounds, Context, CursorStyle, Entity, FocusHandle, MouseButton, MouseMoveEvent,
    Pixels, Subscription, Window, WindowControlArea, div, prelude::*, px,
};
use gpui_libghostty::TerminalTheme;
use menu::ContextMenu;
use sidebar::{SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_SHARE, SIDEBAR_MIN_WIDTH};
use std::collections::HashMap;

mod layout;
mod menu;
mod prefs;
mod sidebar;
mod tools;

#[derive(Clone, Copy, PartialEq)]
enum RenameTarget {
    Tab(TabId),
    Group(GroupId),
}

/// What a pane shows. The layout model only knows `PaneId`s; native surfaces exist only
/// for terminals, so hiding and freezing apply to them alone.
#[derive(Clone)]
enum PaneView {
    Terminal(Entity<TerminalPane>),
    Settings(Entity<SettingsPane>),
}

impl PaneView {
    fn terminal(&self) -> Option<&Entity<TerminalPane>> {
        match self {
            Self::Terminal(pane) => Some(pane),
            Self::Settings(_) => None,
        }
    }

    /// The pane header and window title.
    fn title(&self, cx: &App) -> String {
        match self {
            Self::Terminal(pane) => pane.read(cx).title(),
            Self::Settings(_) => "Settings".into(),
        }
    }

    /// The sidebar card's first line.
    fn name(&self, cx: &App) -> String {
        match self {
            Self::Terminal(pane) => pane.read(cx).name(),
            Self::Settings(_) => "Settings".into(),
        }
    }

    /// The sidebar card's second line.
    fn location(&self, cx: &App) -> Option<String> {
        self.terminal().map(|pane| pane.read(cx).display_cwd())
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        match self {
            Self::Terminal(pane) => pane.update(cx, |pane, cx| pane.focus(window, cx)),
            Self::Settings(pane) => pane.update(cx, |pane, cx| pane.focus(window, cx)),
        }
    }

    fn view(&self) -> AnyView {
        match self {
            Self::Terminal(pane) => pane.clone().into(),
            Self::Settings(pane) => pane.clone().into(),
        }
    }
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
    resizing_sidebar: bool,
    tools_open: bool,
    tools_width: f32,
    /// The tools panel's left edge, recorded on render for resizing.
    tools_left: f32,
    resizing_tools: bool,
    tool: tools::Tool,
    explorer: Entity<Explorer>,
    _explorer_events: Subscription,
    global_search: Entity<SearchPanel>,
    _search_events: Subscription,
    /// Tabs being dragged in the sidebar; their rows show an empty slot meanwhile.
    dragging: Vec<TabId>,
    menu: Option<ContextMenu>,
    /// The shown surfaces are swapped for snapshots while a pane menu covers them.
    frozen: bool,
    renaming: Option<(RenameTarget, Entity<RenameInput>, Subscription)>,
    /// The sidebar's tab filter: the field plus its event and change subscriptions.
    search: Option<(Entity<RenameInput>, [Subscription; 2])>,
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
    _config_watch: Option<(notify::RecommendedWatcher, gpui::Task<()>)>,
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
        let theme = Theme::get(settings.theme().unwrap_or(if dark_appearance {
            ThemeName::Dark
        } else {
            ThemeName::Light
        }));
        let explorer = cx.new(|cx| Explorer::new(theme, cx));
        let explorer_events = cx.subscribe_in(&explorer, window, |view, _, event, window, cx| {
            view.on_explorer_event(event, window, cx)
        });
        let global_search = cx.new(|cx| SearchPanel::new(theme, window, cx));
        let search_events =
            cx.subscribe_in(&global_search, window, |view, _, event, window, cx| {
                view.on_search_event(event, window, cx)
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
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            resizing_sidebar: false,
            tools_open: false,
            tools_width: tools::TOOLS_DEFAULT_WIDTH,
            tools_left: 0.0,
            resizing_tools: false,
            tool: tools::Tool::Explorer,
            explorer,
            _explorer_events: explorer_events,
            global_search,
            _search_events: search_events,
            dragging: Vec::new(),
            menu: None,
            frozen: false,
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
            // ponytail: compares the encoded layout every 2s instead of tracking each
            // change; switch to change notifications if the layout ever gets large.
            config_text,
            dark_appearance,
            _appearance: cx.observe_window_appearance(window, |view, window, cx| {
                view.dark_appearance = prefs::is_dark(window.appearance());
                view.apply_settings(cx);
            }),
            _config_watch: None,
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
        match saved {
            Some(saved) => view.restore(saved, window, cx),
            None => {
                let cwd = view.config.cwd.clone();
                view.open_pane(first, cwd, window, cx);
            }
        }
        view.watch_config(cx);
        view.sync(window, cx);
        view
    }

    /// Reopens a saved layout: terminals start fresh shells in their last directory (home
    /// if it is gone); programs that were running are not restarted.
    fn restore(&mut self, saved: SavedState, window: &mut Window, cx: &mut Context<Self>) {
        self.model = saved.workspace;
        self.sidebar_open = saved.sidebar_open;
        self.sidebar_width = saved.sidebar_width;
        self.tools_width = saved.tools_width;
        if saved.tools_open {
            self.tools_open = true;
            self.explorer
                .update(cx, |explorer, cx| explorer.set_active(true, cx));
        }
        let kinds: HashMap<PaneId, SavedPane> = saved.panes.into_iter().collect();
        let panes: Vec<PaneId> = self.model.panes().collect();
        for id in panes {
            match kinds.get(&id) {
                Some(SavedPane::Settings) => self.insert_settings_pane(id, window, cx),
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
            window: self.window_bounds,
        }
    }

    /// Writes the layout if it changed, through a temporary file so a crash mid-write
    /// leaves the previous state intact.
    fn save_state(&mut self, cx: &App) {
        let mut state = self.state(cx);
        state.panes.sort_by_key(|(id, _)| *id);
        let text = state::encode(&state);
        if text == self.saved {
            return;
        }
        let Some(path) = app::state_file() else {
            return;
        };
        let temporary = path.with_extension("tmp");
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&temporary, &text))
            .and_then(|()| std::fs::rename(&temporary, &path));
        match written {
            Ok(()) => self.saved = text,
            Err(error) => eprintln!("bareframe: saving {}: {error}", path.display()),
        }
    }

    fn terminal_theme(&self) -> TerminalTheme {
        self.theme().terminal(&self.settings)
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
                    PaneEvent::TitleChanged => {
                        view.refresh_explorer_roots(cx);
                        cx.notify()
                    }
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

    fn terminals(&self) -> impl Iterator<Item = (&PaneId, &Entity<TerminalPane>)> {
        self.panes
            .iter()
            .filter_map(|(id, (pane, _))| Some((id, pane.terminal()?)))
    }

    fn pane_title(&self, id: PaneId, cx: &Context<Self>) -> String {
        self.pane(id)
            .map_or_else(String::new, |pane| pane.title(cx))
    }

    /// The active pane's cwd comes from OSC 7, which any local program's output can send
    /// (Ghostty already rejects remote hosts), so only an existing directory is trusted.
    fn active_cwd(&self, cx: &Context<Self>) -> std::path::PathBuf {
        self.terminal(self.model.active_pane())
            .and_then(|pane| pane.read(cx).cwd().canonicalize().ok())
            .filter(|cwd| cwd.is_dir())
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
        for id in self.shown_panes() {
            if let Some(pane) = self.terminal(id) {
                pane.update(cx, |pane, cx| pane.set_frozen(frozen, cx));
            }
        }
    }

    /// Shows only the active tab's surfaces and gives the active pane keyboard focus.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.frozen {
            self.dismiss_menu(cx);
        }
        let visible = self.shown_panes();
        for (id, pane) in self.terminals() {
            let shown = visible.contains(id);
            pane.update(cx, |pane, cx| pane.set_visible(shown, cx));
        }
        if let Some(pane) = self.pane(self.model.active_pane()).cloned() {
            pane.focus(window, cx);
        }
        self.split_bounds.clear();
        self.refresh_explorer_roots(cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = match self.settings.get("working-directory") {
            "home" => app::home_dir().unwrap_or_else(|| self.config.cwd.clone()),
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

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.active_cwd(cx);
        let pane = self.model.split(axis);
        self.open_pane(pane, cwd, window, cx);
        self.sync(window, cx);
    }

    fn close_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
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

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let theme = self.terminal_theme();
        let ui = self.theme();
        for (pane, _) in self.panes.values() {
            match pane {
                PaneView::Terminal(pane) => {
                    pane.update(cx, |pane, cx| pane.apply_theme(&theme, cx))
                }
                PaneView::Settings(pane) => pane.update(cx, |pane, cx| {
                    pane.set_theme(ui, cx);
                    pane.set_settings(self.settings.clone(), cx);
                }),
            }
        }
        self.explorer
            .update(cx, |explorer, cx| explorer.set_theme(ui, cx));
        self.global_search
            .update(cx, |search, cx| search.set_theme(ui, cx));
        cx.notify();
    }

    fn set_theme(&mut self, name: ThemeName, cx: &mut Context<Self>) {
        let value = match name {
            ThemeName::Dark => "dark",
            ThemeName::Light => "light",
        };
        self.set_setting("theme", value, cx);
    }

    fn step_font_size(&mut self, steps: f32, cx: &mut Context<Self>) {
        self.settings.step_font_size(steps);
        self.apply_settings(cx);
    }

    fn drag_divider(&mut self, event: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
        if self.drag_tools_edge(event, window) {
            cx.notify();
            return;
        }
        if self.resizing_sidebar {
            if !event.dragging() {
                self.resizing_sidebar = false;
                return;
            }
            let max = (f32::from(window.viewport_size().width) * SIDEBAR_MAX_SHARE)
                .max(SIDEBAR_MIN_WIDTH);
            self.sidebar_width = f32::from(event.position.x).clamp(SIDEBAR_MIN_WIDTH, max);
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

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        // Quitting and closing the window both land here, before the panes go away.
        if !self.panes.is_empty() {
            self.save_state(cx);
        }
        for (_, (pane, _)) in self.panes.drain() {
            if let PaneView::Terminal(pane) = pane {
                pane.update(cx, |pane, cx| pane.shutdown(cx));
            }
        }
    }

    /// Full-width bar under the traffic lights. The middle is reserved for a search box.
    /// It stands in for the hidden system title bar: drag moves the window, double-click zooms.
    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let open = self.sidebar_open;
        div()
            .id("top-bar")
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.should_move_window = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.should_move_window = false),
            )
            .on_mouse_move(cx.listener(|view, _, window, _| {
                if view.should_move_window {
                    view.should_move_window = false;
                    window.start_window_move();
                }
            }))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .flex()
            .flex_shrink_0()
            .items_center()
            .h(px(app::TITLE_BAR_HEIGHT))
            // Leaves room for the traffic lights drawn over the transparent title bar.
            .pl(px(80.0))
            .pr_2()
            .bg(hsla(theme.chrome))
            .border_b_1()
            .border_color(hsla(theme.border))
            .child(
                div()
                    .id("toggle-sidebar")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .when(open, |this| this.bg(hsla(theme.tab_active)))
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(|view, _, _, cx| {
                        cx.stop_propagation();
                        view.toggle_sidebar(cx)
                    }))
                    .child(sidebar_icon(if open {
                        theme.text
                    } else {
                        theme.text_muted
                    })),
            )
            .child(
                div()
                    .id("toggle-tools")
                    // Hidden when Settings has switched every tool off.
                    .when(self.shown_tool().is_none(), |this| this.invisible())
                    .ml_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .when(self.tools_open, |this| this.bg(hsla(theme.tab_active)))
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(|view, _, window, cx| {
                        cx.stop_propagation();
                        view.toggle_tools(window, cx)
                    }))
                    .child(
                        gpui::svg()
                            .path("icons/wrench.svg")
                            .size(px(14.0))
                            .text_color(hsla(if self.tools_open {
                                theme.text
                            } else {
                                theme.text_muted
                            })),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("open-settings")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(|view, _, window, cx| {
                        cx.stop_propagation();
                        view.open_settings(window, cx)
                    }))
                    .child(
                        gpui::svg()
                            .path("icons/gear.svg")
                            .size(px(15.0))
                            .text_color(hsla(theme.text_muted)),
                    ),
            )
    }

    /// Re-clamped on every render, so shrinking the window can't squeeze the terminals
    /// behind a sidebar sized for a wider one.
    fn sidebar_shown_width(&self, window: &Window) -> f32 {
        self.sidebar_width
            .min(f32::from(window.viewport_size().width) * SIDEBAR_MAX_SHARE)
            .max(SIDEBAR_MIN_WIDTH)
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
            self.sidebar_shown_width(window) + 5.0
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
                    view.resizing_sidebar = false;
                    view.resizing_tools = false;
                    if !view.dragging.is_empty() {
                        view.dragging.clear();
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(|view, _: &ToggleSidebar, _, cx| view.toggle_sidebar(cx)))
            .on_action(
                cx.listener(|view, _: &OpenSettings, window, cx| view.open_settings(window, cx)),
            )
            .on_action(
                cx.listener(|view, _: &ToggleToolsPanel, window, cx| view.toggle_tools(window, cx)),
            )
            .on_action(cx.listener(|view, _: &ShowProjectExplorer, window, cx| {
                view.show_tool(tools::Tool::Explorer, window, cx)
            }))
            .on_action(cx.listener(|view, _: &ShowGlobalSearch, window, cx| {
                view.show_tool(tools::Tool::Search, window, cx)
            }))
            .on_action(cx.listener(|view, _: &FindInFiles, window, cx| {
                view.open_tool(tools::Tool::Search, window, cx)
            }))
            .on_action(cx.listener(|view, _: &CloseWindow, window, cx| {
                view.shutdown(cx);
                window.remove_window();
            }))
            .on_action(cx.listener(|view, _: &ClosePane, window, cx| {
                let pane = view.model.active_pane();
                view.close_pane(pane, window, cx)
            }))
            .on_action(cx.listener(|view, _: &RestartSession, window, cx| {
                let theme = view.terminal_theme();
                if let Some(pane) = view.terminal(view.model.active_pane()) {
                    pane.update(cx, |pane, cx| pane.retry(&theme, window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &NewTab, window, cx| view.new_tab(window, cx)))
            .on_action(cx.listener(|view, _: &NextTab, window, cx| {
                view.model.cycle_tab(true);
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|view, _: &PreviousTab, window, cx| {
                view.model.cycle_tab(false);
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|view, _: &SelectTab1, w, cx| view.select_tab(0, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab2, w, cx| view.select_tab(1, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab3, w, cx| view.select_tab(2, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab4, w, cx| view.select_tab(3, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab5, w, cx| view.select_tab(4, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab6, w, cx| view.select_tab(5, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab7, w, cx| view.select_tab(6, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab8, w, cx| view.select_tab(7, w, cx)))
            .on_action(cx.listener(|view, _: &SelectLastTab, window, cx| {
                let last = view.model.tabs.len() - 1;
                view.select_tab(last, window, cx)
            }))
            .on_action(cx.listener(|view, _: &SplitRight, window, cx| {
                view.split(Axis::Horizontal, window, cx)
            }))
            .on_action(
                cx.listener(|view, _: &SplitDown, window, cx| {
                    view.split(Axis::Vertical, window, cx)
                }),
            )
            .on_action(cx.listener(|view, _: &ToggleMaximizePane, window, cx| {
                view.model.toggle_maximized();
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|view, _: &FocusLeft, w, cx| {
                view.focus_direction(Direction::Left, w, cx)
            }))
            .on_action(cx.listener(|view, _: &FocusRight, w, cx| {
                view.focus_direction(Direction::Right, w, cx)
            }))
            .on_action(
                cx.listener(|view, _: &FocusUp, w, cx| view.focus_direction(Direction::Up, w, cx)),
            )
            .on_action(cx.listener(|view, _: &FocusDown, w, cx| {
                view.focus_direction(Direction::Down, w, cx)
            }))
            .on_action(
                cx.listener(|view, _: &IncreaseFontSize, _, cx| view.step_font_size(1.0, cx)),
            )
            .on_action(
                cx.listener(|view, _: &DecreaseFontSize, _, cx| view.step_font_size(-1.0, cx)),
            )
            .on_action(cx.listener(|view, _: &ResetFontSize, _, cx| {
                let base = view.base_font_size.to_string();
                if let Err(error) = view.settings.set("font-size", &base) {
                    eprintln!("bareframe: {error}");
                }
                view.apply_settings(cx)
            }))
            .on_action(
                cx.listener(|view, _: &UseDarkTheme, _, cx| view.set_theme(ThemeName::Dark, cx)),
            )
            .on_action(
                cx.listener(|view, _: &UseLightTheme, _, cx| view.set_theme(ThemeName::Light, cx)),
            )
            // Both reorder tabs, so `sync` also closes any menu holding tab indices.
            .on_action(cx.listener(|view, _: &GroupTab, window, cx| {
                let tabs = view.model.target_tabs();
                view.model.new_group(&tabs);
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|view, _: &UngroupTab, window, cx| {
                let tabs = view.model.target_tabs();
                view.model.remove_from_group(&tabs);
                view.sync(window, cx)
            }))
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
            .child(self.render_top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .when(self.sidebar_open, |this| {
                        let width = self.sidebar_shown_width(window);
                        this.child(self.render_sidebar(width, cx)).child(
                            // Drag handle on the sidebar's right edge. The line sits flush with
                            // the sidebar so cards keep equal left and right margins.
                            div()
                                .flex_none()
                                .w(px(5.0))
                                .h_full()
                                .flex()
                                .bg(hsla(theme.terminal_background))
                                .cursor(CursorStyle::ResizeLeftRight)
                                .child(div().w(px(1.0)).h_full().bg(hsla(theme.border)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|view, _, _, cx| {
                                        cx.stop_propagation();
                                        view.resizing_sidebar = true;
                                    }),
                                ),
                        )
                    })
                    .when(self.tools_open, |this| {
                        let width = self.tools_shown_width(window);
                        this.child(self.render_tools(width, cx))
                            .child(self.render_tools_edge(cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().child(content)),
            )
    }
}

fn icon_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded_sm()
        .text_color(hsla(theme.text_muted))
        .hover(|style| style.bg(hsla(theme.border)).text_color(hsla(theme.text)))
        .child(label)
}

/// Outline of a window with its left panel filled, drawn so no icon font is needed.
fn sidebar_icon(color: u32) -> impl IntoElement {
    div()
        .flex()
        .w(px(15.0))
        .h(px(12.0))
        .rounded(px(2.5))
        .border_1()
        .border_color(hsla(color))
        .child(div().w(px(4.5)).h_full().bg(hsla(color)))
}
