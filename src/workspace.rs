use crate::{
    app::{self, *},
    pane::{PaneEvent, TerminalPane},
    theme::{self, Theme, hsla},
};
use bareframe_core::{
    config::LaunchConfig,
    settings::{Settings, ThemeName},
    workspace::{Axis, Closed, Direction, GroupId, Layout, PaneId, Tab, Workspace},
};
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Entity, FocusHandle, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, Subscription, Window, WindowControlArea, anchored, canvas,
    deferred, div, prelude::*, px, relative,
};
use gpui_libghostty::TerminalTheme;
use std::collections::HashMap;

/// Sidebar width bounds follow Warp's vertical tabs panel.
const SIDEBAR_DEFAULT_WIDTH: f32 = 248.0;
const SIDEBAR_MIN_WIDTH: f32 = 200.0;
const SIDEBAR_MAX_SHARE: f32 = 0.5;

/// A tab being dragged in the sidebar; also renders the drag preview.
#[derive(Clone)]
struct DraggedTab {
    index: usize,
    title: String,
    background: u32,
    text: u32,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(hsla(self.background))
            .text_color(hsla(self.text))
            .font_family(theme::UI_FONT)
            .text_size(px(12.5))
            .child(self.title.clone())
    }
}

/// Width of sidebar context menus; they must fit inside the sidebar because the
/// terminal's native surface covers anything drawn over it.
const MENU_WIDTH: f32 = 188.0;

#[derive(Clone)]
enum MenuAction {
    NewGroup(Vec<usize>),
    MoveToGroup(Vec<usize>, GroupId),
    RemoveFromGroup(Vec<usize>),
    CloseTabs(Vec<usize>),
    CloseOtherTabs(usize),
    NewTabInGroup(GroupId),
    Ungroup(GroupId),
    CloseGroup(GroupId),
    ToggleGroup(GroupId),
}

struct ContextMenu {
    position: Point<Pixels>,
    items: Vec<(String, MenuAction)>,
}

struct Resize {
    tab: usize,
    path: Vec<bool>,
    axis: Axis,
}

/// Owns tabs, layout and the panes; never a PTY or native surface directly.
pub struct WorkspaceView {
    model: Workspace,
    panes: HashMap<PaneId, (Entity<TerminalPane>, Subscription)>,
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
    menu: Option<ContextMenu>,
    focus: FocusHandle,
    _quit: Subscription,
}

impl WorkspaceView {
    pub fn new(
        config: LaunchConfig,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (model, first) = Workspace::new();
        let mut view = Self {
            model,
            panes: HashMap::new(),
            base_font_size: settings.font_size,
            settings,
            config,
            resize: None,
            split_bounds: HashMap::new(),
            should_move_window: false,
            sidebar_open: true,
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            resizing_sidebar: false,
            menu: None,
            focus: cx.focus_handle(),
            _quit: cx.on_app_quit(|view, cx| {
                view.shutdown(cx);
                async {}
            }),
        };
        let cwd = view.config.cwd.clone();
        view.open_pane(first, cwd, window, cx);
        view
    }

    fn theme(&self) -> &'static Theme {
        Theme::get(self.settings.theme)
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
        let config = LaunchConfig {
            cwd,
            ..self.config.clone()
        };
        let theme = self.terminal_theme();
        let pane = cx.new(|cx| TerminalPane::new(id, config, &theme, window, cx));
        let subscription =
            cx.subscribe_in(
                &pane,
                window,
                move |view, _, event, window, cx| match event {
                    PaneEvent::Exited => view.close_pane(id, window, cx),
                    PaneEvent::TitleChanged => cx.notify(),
                },
            );
        self.panes.insert(id, (pane, subscription));
    }

    /// The active pane's cwd comes from OSC 7, which any local program's output can send
    /// (Ghostty already rejects remote hosts), so only an existing directory is trusted.
    fn active_cwd(&self, cx: &Context<Self>) -> std::path::PathBuf {
        self.panes
            .get(&self.model.active_pane())
            .and_then(|(pane, _)| pane.read(cx).cwd().canonicalize().ok())
            .filter(|cwd| cwd.is_dir())
            .unwrap_or_else(|| self.config.cwd.clone())
    }

    /// Shows only the active tab's surfaces and gives the active pane keyboard focus.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible: Vec<PaneId> = self.model.active().layout.panes();
        for (id, (pane, _)) in &self.panes {
            let shown = visible.contains(id);
            pane.update(cx, |pane, cx| pane.set_visible(shown, cx));
        }
        if let Some((pane, _)) = self.panes.get(&self.model.active_pane()) {
            pane.update(cx, |pane, cx| pane.focus(window, cx));
        }
        self.split_bounds.clear();
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.active_cwd(cx);
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
        if let Some((pane, _)) = self.panes.remove(&id) {
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

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.model.tabs.get(index) else {
            return;
        };
        for pane in tab.layout.panes() {
            self.close_pane(pane, window, cx);
        }
    }

    fn close_group(&mut self, group: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        let panes: Vec<PaneId> = self
            .model
            .group_tabs(group)
            .into_iter()
            .flat_map(|index| self.model.tabs[index].layout.panes())
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

    /// Drops `dragged` before the tab at `to`, joining `group`.
    fn drop_tab(
        &mut self,
        dragged: &DraggedTab,
        to: usize,
        group: Option<GroupId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.move_tab(dragged.index, to, group);
        self.sync(window, cx);
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
        for (pane, _) in self.panes.values() {
            pane.update(cx, |pane, cx| pane.apply_theme(&theme, cx));
        }
        cx.notify();
    }

    fn set_theme(&mut self, name: ThemeName, cx: &mut Context<Self>) {
        self.settings.theme = name;
        self.apply_settings(cx);
    }

    fn step_font_size(&mut self, steps: f32, cx: &mut Context<Self>) {
        self.settings.step_font_size(steps);
        self.apply_settings(cx);
    }

    fn drag_divider(&mut self, event: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
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
        for (_, (pane, _)) in self.panes.drain() {
            pane.update(cx, |pane, cx| pane.shutdown(cx));
        }
    }

    /// Plain click focuses a tab; shift extends the selection from the active tab and
    /// cmd toggles one tab, as in Warp's vertical tabs.
    fn click_tab(
        &mut self,
        index: usize,
        modifiers: gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if modifiers.shift {
            self.model.select_range(index);
            cx.notify();
        } else if modifiers.platform {
            self.model.toggle_selected(index);
            cx.notify();
        } else {
            self.model.clear_selection();
            self.select_tab(index, window, cx);
        }
    }

    fn open_menu(
        &mut self,
        position: Point<Pixels>,
        items: Vec<(String, MenuAction)>,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(position.x)
            .min(self.sidebar_width - MENU_WIDTH - 4.0)
            .max(4.0);
        self.menu = Some(ContextMenu {
            position: gpui::point(px(x), position.y),
            items,
        });
        cx.notify();
    }

    fn group_label(&self, id: GroupId) -> String {
        self.model
            .group(id)
            .map_or_else(|| "group".into(), |group| group.name.clone())
    }

    /// Right-clicking a tab in a multi-selection acts on the selection; any other
    /// tab clears the selection and gets its own menu.
    fn open_tab_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let selection = self.model.selected_indices();
        let targets = if selection.len() > 1 && selection.contains(&index) {
            selection
        } else {
            self.model.clear_selection();
            vec![index]
        };
        let mut items = Vec::new();
        if targets.len() > 1 {
            items.push((
                "Create group from tabs".into(),
                MenuAction::NewGroup(targets.clone()),
            ));
        } else {
            items.push((
                "New group with tab".into(),
                MenuAction::NewGroup(targets.clone()),
            ));
        }
        for group in self.model.destination_groups(&targets) {
            items.push((
                format!("Move to {}", self.group_label(group)),
                MenuAction::MoveToGroup(targets.clone(), group),
            ));
        }
        if self.model.shared_group(&targets).is_some() {
            items.push((
                "Remove from group".into(),
                MenuAction::RemoveFromGroup(targets.clone()),
            ));
        }
        if targets.len() > 1 {
            items.push((
                format!("Close {} tabs", targets.len()),
                MenuAction::CloseTabs(targets),
            ));
        } else {
            items.push(("Close tab".into(), MenuAction::CloseTabs(targets)));
            if self.model.tabs.len() > 1 {
                items.push(("Close other tabs".into(), MenuAction::CloseOtherTabs(index)));
            }
        }
        self.open_menu(position, items, cx);
    }

    fn open_group_menu(&mut self, id: GroupId, position: Point<Pixels>, cx: &mut Context<Self>) {
        let collapsed = self.model.group(id).is_some_and(|group| group.collapsed);
        self.model.clear_selection();
        let items = vec![
            ("New tab in group".into(), MenuAction::NewTabInGroup(id)),
            (
                if collapsed {
                    "Expand group"
                } else {
                    "Collapse group"
                }
                .into(),
                MenuAction::ToggleGroup(id),
            ),
            ("Ungroup tabs".into(), MenuAction::Ungroup(id)),
            ("Close all tabs in group".into(), MenuAction::CloseGroup(id)),
        ];
        self.open_menu(position, items, cx);
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

    fn run_menu_action(&mut self, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        match action {
            MenuAction::NewGroup(tabs) => {
                self.model.new_group(&tabs);
            }
            MenuAction::MoveToGroup(tabs, group) => self.model.move_to_group(&tabs, group),
            MenuAction::RemoveFromGroup(tabs) => self.model.remove_from_group(&tabs),
            MenuAction::CloseTabs(tabs) => self.close_tabs(&tabs, window, cx),
            MenuAction::CloseOtherTabs(keep) => {
                let others: Vec<usize> =
                    (0..self.model.tabs.len()).filter(|&i| i != keep).collect();
                self.close_tabs(&others, window, cx);
            }
            MenuAction::NewTabInGroup(group) => self.new_tab_in_group(group, window, cx),
            MenuAction::Ungroup(group) => {
                let tabs = self.model.group_tabs(group);
                self.model.remove_from_group(&tabs);
            }
            MenuAction::CloseGroup(group) => self.close_group(group, window, cx),
            MenuAction::ToggleGroup(group) => self.model.toggle_group(group),
        }
        self.sync(window, cx);
    }

    fn render_menu(&self, menu: &ContextMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let items = menu
            .items
            .iter()
            .enumerate()
            .map(|(index, (label, action))| {
                let action = action.clone();
                div()
                    .id(("menu-item", index))
                    .px_2()
                    .py(px(5.0))
                    .mx_1()
                    .rounded_sm()
                    .truncate()
                    .hover(|style| style.bg(hsla(theme.accent)).text_color(hsla(theme.chrome)))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.run_menu_action(action.clone(), window, cx)
                    }))
                    .child(label.clone())
            });
        deferred(
            anchored().position(menu.position).snap_to_window().child(
                div()
                    .id("context-menu")
                    // Without this, clicks on menu items also reach the tab rows underneath.
                    .occlude()
                    .w(px(MENU_WIDTH))
                    .py_1()
                    .flex()
                    .flex_col()
                    .bg(hsla(theme.tab_active))
                    .border_1()
                    .border_color(hsla(theme.border))
                    .rounded_md()
                    .shadow_lg()
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.5))
                    .text_color(hsla(theme.text))
                    .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                        view.menu = None;
                        cx.notify()
                    }))
                    .children(items),
            ),
        )
        .with_priority(1)
    }

    fn tab_title(&self, tab: &Tab, cx: &Context<Self>) -> String {
        self.panes
            .get(&tab.active_pane)
            .map_or_else(String::new, |(pane, _)| pane.read(cx).title())
    }

    /// The second line of a tab row: the `~` path, unless the title already shows it.
    fn tab_subtitle(&self, tab: &Tab, title: &str, cx: &Context<Self>) -> Option<String> {
        let (pane, _) = self.panes.get(&tab.active_pane)?;
        Some(pane.read(cx).display_cwd()).filter(|path| path != title)
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let mut rows = Vec::new();
        let mut previous = None;
        for (index, tab) in self.model.tabs.iter().enumerate() {
            if tab.group != previous {
                if let Some(group) = tab.group {
                    rows.push(self.render_group_header(group, cx));
                }
                previous = tab.group;
            }
            let collapsed = tab
                .group
                .and_then(|group| self.model.group(group))
                .is_some_and(|group| group.collapsed);
            if !collapsed {
                rows.push(self.render_tab_row(index, tab, cx));
            }
        }
        let end = self.model.tabs.len();
        div()
            .w(px(self.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.chrome))
            .font_family(theme::UI_FONT)
            .text_size(px(12.5))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_end()
                    .h(px(34.0))
                    .px_2()
                    .child(
                        icon_button("new-tab", "+", theme)
                            .text_size(px(16.0))
                            .on_click(cx.listener(|view, _, window, cx| {
                                cx.stop_propagation();
                                view.new_tab(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("tab-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .px_2()
                    .children(rows)
                    // Dropping below the list moves a tab to the end, outside any group.
                    .child(
                        div()
                            .id("tab-list-end")
                            .flex_1()
                            .min_h(px(32.0))
                            .rounded_md()
                            .drag_over::<DraggedTab>(move |style, _, _, _| {
                                style.bg(hsla(theme.tab_hover))
                            })
                            .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                                view.drop_tab(dragged, end, None, window, cx)
                            })),
                    ),
            )
    }

    /// Stands in for the hidden system title bar: drag moves the window, double-click zooms.
    fn title_bar(&self, id: &'static str, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
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
    }

    /// Full-width bar under the traffic lights. The middle is reserved for a search box.
    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let open = self.sidebar_open;
        self.title_bar("top-bar", cx)
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
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    fn render_group_header(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let Some(group) = self.model.group(id) else {
            return div().into_any_element();
        };
        let tabs = self.model.group_tabs(id);
        let end = tabs.last().map_or(0, |last| last + 1);
        let hover = format!("group-{}", id.0);
        let color = hsla(theme.group_colors[group.color]);
        // A collapsed group hides the active tab's row, so the header stands in for it.
        let holds_active = group.collapsed && tabs.contains(&self.model.active_tab);
        div()
            .id(("group", id.0))
            .group(hover.clone())
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .mt_1()
            .px_2()
            .rounded_md()
            .text_color(hsla(theme.text))
            .when(holds_active, |this| this.bg(hsla(theme.tab_active)))
            .hover(|style| style.bg(hsla(theme.tab_hover)))
            .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(hsla(theme.tab_active)))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.model.toggle_group(id);
                cx.notify()
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.open_group_menu(id, event.position, cx)
                }),
            )
            .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                view.drop_tab(dragged, end, Some(id), window, cx)
            }))
            .child(
                div()
                    .w(px(10.0))
                    .text_color(hsla(theme.text_muted))
                    .child(if group.collapsed { "▸" } else { "▾" }),
            )
            .child(div().size(px(8.0)).rounded_full().bg(color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(group.name.clone()),
            )
            .child(
                div()
                    .text_color(hsla(theme.text_muted))
                    .group_hover(hover.clone(), |style| style.invisible())
                    .child(tabs.len().to_string()),
            )
            .child(
                div()
                    .flex()
                    .invisible()
                    .group_hover(hover, |style| style.visible())
                    .child(
                        icon_button(("group-new-tab", id.0), "+", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.new_tab_in_group(id, window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(("group-close", id.0), "×", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_group(id, window, cx)
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    fn render_tab_row(&self, index: usize, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let active = index == self.model.active_tab;
        let title = self.tab_title(tab, cx);
        let subtitle = self.tab_subtitle(tab, &title, cx);
        // Selection highlight only matters once more than one tab is involved.
        let selection = self.model.selected_indices();
        let selected = selection.len() > 1 && selection.contains(&index);
        let group = tab.group;
        let color = group
            .and_then(|group| self.model.group(group))
            .map(|group| hsla(theme.group_colors[group.color]));
        let hover = format!("tab-{index}");
        let dragged = DraggedTab {
            index,
            title: title.clone(),
            background: theme.tab_active,
            text: theme.text,
        };
        div()
            .id(("tab", index))
            .group(hover.clone())
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .min_h(px(30.0))
            .py(px(5.0))
            .pl(px(if group.is_some() { 18.0 } else { 10.0 }))
            .pr_1()
            .rounded_md()
            .text_color(hsla(if active || selected {
                theme.text
            } else {
                theme.text_muted
            }))
            .when(active, |this| this.bg(hsla(theme.tab_active)))
            .when(selected, |this| {
                this.bg(hsla(theme.accent).opacity(0.22))
                    .border_1()
                    .border_color(hsla(theme.accent).opacity(0.5))
            })
            .when(!active && !selected, |this| {
                this.hover(|style| style.bg(hsla(theme.tab_hover)))
            })
            .on_click(
                cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
                    view.click_tab(index, event.modifiers(), window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.open_tab_menu(index, event.position, cx)
                }),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<DraggedTab>(move |style, _, _, _| {
                style.border_t_2().border_color(hsla(theme.accent))
            })
            .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                view.drop_tab(dragged, index, group, window, cx)
            }))
            .when_some(color, |this, color| {
                this.child(div().w(px(3.0)).h(px(14.0)).rounded_sm().bg(color))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().child(title))
                    .when_some(subtitle, |this, subtitle| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(hsla(theme.text_muted))
                                .child(subtitle),
                        )
                    }),
            )
            .child(
                icon_button(("close-tab", index), "×", theme)
                    .when(!active, |this| {
                        this.invisible().group_hover(hover, |style| style.visible())
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.close_tab(index, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn render_layout(
        &self,
        layout: &Layout,
        path: &mut Vec<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme();
        match layout {
            Layout::Pane(id) => {
                let id = *id;
                let active =
                    id == self.model.active_pane() && self.model.active().layout.panes().len() > 1;
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    // Breathing room around the text; the surface can't be padded from GPUI.
                    .p(px(6.0))
                    .border_1()
                    .border_color(hsla(if active {
                        theme.accent
                    } else {
                        theme.terminal_background
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, window, cx| {
                            if view.model.active_pane() != id && view.model.focus(id) {
                                view.sync(window, cx);
                            }
                        }),
                    )
                    .children(self.panes.get(&id).map(|(pane, _)| pane.clone()))
                    .into_any_element()
            }
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = *axis;
                let here = path.clone();
                path.push(false);
                let first = self.render_layout(first, path, cx);
                path.pop();
                path.push(true);
                let second = self.render_layout(second, path, cx);
                path.pop();
                let weak = cx.entity().downgrade();
                let recorded = here.clone();
                let tab = self.model.active_tab;
                let divider = div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .items_center()
                    .bg(hsla(theme.terminal_background))
                    .map(|this| match axis {
                        Axis::Horizontal => this
                            .w(px(5.0))
                            .h_full()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .child(div().w(px(1.0)).h_full().bg(hsla(theme.border))),
                        Axis::Vertical => this
                            .h(px(5.0))
                            .w_full()
                            .cursor(CursorStyle::ResizeUpDown)
                            .child(div().h(px(1.0)).w_full().bg(hsla(theme.border))),
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.resize = Some(Resize {
                                tab,
                                path: here.clone(),
                                axis,
                            });
                        }),
                    );
                div()
                    .relative()
                    .flex()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .map(|this| match axis {
                        Axis::Horizontal => this.flex_row(),
                        Axis::Vertical => this.flex_col(),
                    })
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                if let Err(error) = weak.update(cx, |view, _| {
                                    view.split_bounds.insert(recorded.clone(), bounds);
                                }) {
                                    eprintln!("bareframe: recording split bounds: {error}");
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .flex_none()
                            .min_w_0()
                            .min_h_0()
                            .map(|this| match axis {
                                Axis::Horizontal => this.h_full().w(relative(*ratio)),
                                Axis::Vertical => this.w_full().h(relative(*ratio)),
                            })
                            .child(first),
                    )
                    .child(divider)
                    .child(div().flex_1().min_w_0().min_h_0().child(second))
                    .into_any_element()
            }
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let title = self
            .panes
            .get(&self.model.active_pane())
            .map_or_else(String::new, |(pane, _)| pane.read(cx).title());
        window.set_window_title(&format!("{title} — {}", app::name()));
        let layout = self.model.active().layout.clone();
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
                cx.listener(|view, _, _, _| {
                    view.resize = None;
                    view.resizing_sidebar = false;
                }),
            )
            .on_action(cx.listener(|view, _: &ToggleSidebar, _, cx| view.toggle_sidebar(cx)))
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
                if let Some((pane, _)) = view.panes.get(&view.model.active_pane()) {
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
                view.settings.font_size = view.base_font_size;
                view.apply_settings(cx)
            }))
            .on_action(
                cx.listener(|view, _: &UseDarkTheme, _, cx| view.set_theme(ThemeName::Dark, cx)),
            )
            .on_action(
                cx.listener(|view, _: &UseLightTheme, _, cx| view.set_theme(ThemeName::Light, cx)),
            )
            .on_action(cx.listener(|view, _: &GroupTab, _, cx| {
                let tabs = view.model.target_tabs();
                view.model.new_group(&tabs);
                cx.notify()
            }))
            .on_action(cx.listener(|view, _: &UngroupTab, _, cx| {
                let tabs = view.model.target_tabs();
                view.model.remove_from_group(&tabs);
                cx.notify()
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
                        this.child(self.render_sidebar(cx)).child(
                            // Drag handle on the sidebar's right edge.
                            div()
                                .flex_none()
                                .w(px(5.0))
                                .h_full()
                                .flex()
                                .justify_center()
                                .bg(hsla(theme.chrome))
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
