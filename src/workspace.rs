use crate::{
    app::{self, *},
    pane::{PaneEvent, TerminalPane},
    rename::{RenameEvent, RenameInput},
    theme::{self, Theme, hsla},
};
use bareframe_core::{
    config::LaunchConfig,
    settings::{Settings, ThemeName},
    workspace::{Axis, Closed, Direction, GroupId, Layout, PaneId, Tab, TabId, Workspace},
};
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Entity, FocusHandle, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, Subscription, Window, WindowControlArea,
    anchored, canvas, deferred, div, prelude::*, px, relative,
};
use gpui_libghostty::TerminalTheme;
use std::collections::HashMap;

/// Sidebar width bounds follow Warp's vertical tabs panel.
const SIDEBAR_DEFAULT_WIDTH: f32 = 248.0;
const SIDEBAR_MIN_WIDTH: f32 = 200.0;
const SIDEBAR_MAX_SHARE: f32 = 0.5;

/// Tabs being dragged in the sidebar (the selection, or one tab); also renders the preview.
#[derive(Clone)]
struct DraggedTab {
    indices: Vec<usize>,
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

#[derive(Clone, Copy, PartialEq)]
enum RenameTarget {
    Tab(TabId),
    Group(GroupId),
}

#[derive(Clone)]
enum MenuAction {
    Rename(RenameTarget),
    NewGroup(Vec<usize>),
    /// Replaces the menu with the destination groups, as Warp's submenu does.
    ShowMoveTargets(Vec<usize>),
    MoveToGroup(Vec<usize>, GroupId),
    RemoveFromGroup(Vec<usize>),
    PinTab(usize, bool),
    PinGroup(GroupId, bool),
    MoveGroup(GroupId, bool),
    CloseTabs(Vec<usize>),
    NewTabInGroup(GroupId),
    Ungroup(GroupId),
    CloseGroup(GroupId),
    ToggleGroup(GroupId),
    ToggleMaximize,
    Split(Axis),
    CopyText(String),
    MoveTab(usize, bool),
    SetTabColor(usize, Option<usize>),
    /// Rendered as a divider line.
    Separator,
    /// Rendered as the row of color dots for this tab.
    TabColors(usize),
}

struct ContextMenu {
    position: Point<Pixels>,
    /// A tab in a label separates the right-aligned shortcut.
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
    /// The shown surfaces are swapped for snapshots while a pane menu covers them.
    frozen: bool,
    renaming: Option<(RenameTarget, Entity<RenameInput>, Subscription)>,
    /// The sidebar's tab filter: the field plus its event and change subscriptions.
    search: Option<(Entity<RenameInput>, [Subscription; 2])>,
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
            frozen: false,
            renaming: None,
            search: None,
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
            if let Some((pane, _)) = self.panes.get(&id) {
                pane.update(cx, |pane, cx| pane.set_frozen(frozen, cx));
            }
        }
    }

    fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        if self.frozen {
            self.set_frozen(false, cx);
        }
        cx.notify();
    }

    /// Shows only the active tab's surfaces and gives the active pane keyboard focus.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.frozen {
            self.dismiss_menu(cx);
        }
        let visible = self.shown_panes();
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
        self.model.move_tabs(&dragged.indices, to, group);
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
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if modifiers.shift {
            self.model.select_range(index);
            cx.notify();
        } else if click_count == 2 {
            if let Some(tab) = self.model.tabs.get(index) {
                self.start_rename(RenameTarget::Tab(tab.id), window, cx);
            }
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
        self.menu = Some(ContextMenu { position, items });
        // Menus may hang over the terminals, which native surfaces would otherwise cover.
        if !self.frozen {
            self.set_frozen(true, cx);
        }
        cx.notify();
    }

    fn start_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }
        let accent = self.theme().accent;
        let input = cx.new(|cx| RenameInput::search(accent, window, cx));
        let events = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            view.finish_search(event, window, cx)
        });
        let changes = cx.observe(&input, |_, _, cx| cx.notify());
        self.search = Some((input, [events, changes]));
        cx.notify();
    }

    /// Enter jumps to the first match; Esc clears the filter.
    fn finish_search(&mut self, event: &RenameEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let RenameEvent::Confirm(_) = event
            && let Some(matches) = self.search_matches(cx)
        {
            let Some(&first) = matches.first() else {
                return;
            };
            self.model.clear_selection();
            self.model.select_tab(first);
        }
        self.search = None;
        self.sync(window, cx);
    }

    /// Tabs whose title or path contains the query, or `None` when not searching.
    fn search_matches(&self, cx: &Context<Self>) -> Option<Vec<usize>> {
        let (input, _) = self.search.as_ref()?;
        let query = input.read(cx).text().trim().to_lowercase();
        if query.is_empty() {
            return None;
        }
        let found = |text: String| text.to_lowercase().contains(&query);
        Some(
            (0..self.model.tabs.len())
                .filter(|&index| {
                    let tab = &self.model.tabs[index];
                    let title = self.tab_title(tab, cx);
                    let path = self
                        .panes
                        .get(&tab.active_pane)
                        .map(|(pane, _)| pane.read(cx).display_cwd());
                    found(title) || path.is_some_and(found)
                })
                .collect(),
        )
    }

    fn group_label(&self, id: GroupId) -> String {
        self.model
            .group(id)
            .and_then(|group| group.name.clone())
            .unwrap_or_else(|| "New Group".into())
    }

    fn start_rename(&mut self, target: RenameTarget, window: &mut Window, cx: &mut Context<Self>) {
        let text = match target {
            RenameTarget::Tab(id) => match self.model.tab_index(id) {
                Some(index) => self.tab_title(&self.model.tabs[index], cx),
                None => return,
            },
            RenameTarget::Group(id) => match self.model.group(id) {
                Some(group) => group.name.clone().unwrap_or_default(),
                None => return,
            },
        };
        let accent = self.theme().accent;
        let input = cx.new(|cx| RenameInput::new(&text, accent, window, cx));
        let subscription = cx.subscribe_in(&input, window, move |view, _, event, window, cx| {
            view.finish_rename(target, event, window, cx)
        });
        self.renaming = Some((target, input, subscription));
        cx.notify();
    }

    fn finish_rename(
        &mut self,
        target: RenameTarget,
        event: &RenameEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.renaming.as_ref().map(|(current, _, _)| *current) != Some(target) {
            return;
        }
        self.renaming = None;
        if let RenameEvent::Confirm(text) = event {
            match target {
                RenameTarget::Tab(id) => self.model.rename_tab(id, text),
                RenameTarget::Group(id) => self.model.rename_group(id, text),
            }
        }
        self.sync(window, cx);
    }

    /// The rename field for `target`, if it is being renamed.
    fn rename_field(&self, target: RenameTarget) -> Option<AnyElement> {
        let (current, input, _) = self.renaming.as_ref()?;
        let theme = self.theme();
        (*current == target).then(|| {
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .px_1()
                .rounded_sm()
                .bg(hsla(theme.terminal_background))
                .border_1()
                .border_color(hsla(theme.accent))
                .text_color(hsla(theme.text))
                .child(input.clone())
                .into_any_element()
        })
    }

    /// Right-clicking a tab in a multi-selection acts on the selection; any other
    /// tab clears the selection and gets its own menu. Sections follow Warp's order.
    fn open_tab_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(tab) = self.model.tabs.get(index) else {
            return;
        };
        let (id, grouped, pane) = (tab.id, tab.group.is_some(), tab.active_pane);
        let selection = self.model.selected_indices();
        let targets = if selection.len() > 1 && selection.contains(&index) {
            selection
        } else {
            self.model.clear_selection();
            vec![index]
        };
        let multiple = targets.len() > 1;
        let mut sections: Vec<Vec<(String, MenuAction)>> = Vec::new();
        if !multiple && !grouped {
            let pinned = self.model.tab_pinned(index);
            sections.push(vec![(
                if pinned { "Unpin tab" } else { "Pin tab" }.into(),
                MenuAction::PinTab(index, !pinned),
            )]);
        }
        let mut grouping = vec![(
            if multiple {
                "Create group from tabs"
            } else {
                "New group with tab"
            }
            .into(),
            MenuAction::NewGroup(targets.clone()),
        )];
        if !self.model.destination_groups(&targets).is_empty() {
            grouping.push((
                "Move to group ›".into(),
                MenuAction::ShowMoveTargets(targets.clone()),
            ));
        }
        if self.model.shared_group(&targets).is_some() {
            grouping.push((
                "Remove from group".into(),
                MenuAction::RemoveFromGroup(targets.clone()),
            ));
        }
        sections.push(grouping);
        let count = self.model.tabs.len();
        if multiple {
            sections.push(vec![(
                format!("Close {} tabs", targets.len()),
                MenuAction::CloseTabs(targets),
            )]);
        } else {
            if let Some((pane, _)) = self.panes.get(&pane) {
                let pane = pane.read(cx);
                sections.push(vec![
                    ("Copy pane title".into(), MenuAction::CopyText(pane.title())),
                    (
                        "Copy working directory".into(),
                        MenuAction::CopyText(pane.cwd().display().to_string()),
                    ),
                ]);
            }
            let mut editing = vec![(
                "Rename tab".into(),
                MenuAction::Rename(RenameTarget::Tab(id)),
            )];
            if self.model.can_move_tab(index, true) {
                editing.push(("Move tab up".into(), MenuAction::MoveTab(index, true)));
            }
            if self.model.can_move_tab(index, false) {
                editing.push(("Move tab down".into(), MenuAction::MoveTab(index, false)));
            }
            sections.push(editing);
            let mut closing = vec![("Close tab".into(), MenuAction::CloseTabs(targets))];
            if count > 1 {
                let others = (0..count).filter(|&i| i != index).collect();
                closing.push(("Close other tabs".into(), MenuAction::CloseTabs(others)));
            }
            if index > 0 {
                closing.push((
                    "Close tabs above".into(),
                    MenuAction::CloseTabs((0..index).collect()),
                ));
            }
            if index + 1 < count {
                closing.push((
                    "Close tabs below".into(),
                    MenuAction::CloseTabs((index + 1..count).collect()),
                ));
            }
            sections.push(closing);
            sections.push(vec![(String::new(), MenuAction::TabColors(index))]);
        }
        let mut items = Vec::new();
        for section in sections {
            if !items.is_empty() {
                items.push((String::new(), MenuAction::Separator));
            }
            items.extend(section);
        }
        self.open_menu(position, items, cx);
    }

    fn open_group_menu(&mut self, id: GroupId, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(group) = self.model.group(id) else {
            return;
        };
        let (collapsed, pinned) = (group.collapsed, group.pinned);
        self.model.clear_selection();
        let members = self.model.group_tabs(id);
        let others: Vec<usize> = (0..self.model.tabs.len())
            .filter(|index| !members.contains(index))
            .collect();
        let mut items: Vec<(String, MenuAction)> = vec![
            ("Rename".into(), MenuAction::Rename(RenameTarget::Group(id))),
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
            (
                if pinned { "Unpin group" } else { "Pin group" }.into(),
                MenuAction::PinGroup(id, !pinned),
            ),
            ("Move group up".into(), MenuAction::MoveGroup(id, true)),
            ("Move group down".into(), MenuAction::MoveGroup(id, false)),
            ("Ungroup tabs".into(), MenuAction::Ungroup(id)),
            ("Close all tabs in group".into(), MenuAction::CloseGroup(id)),
        ];
        if !others.is_empty() {
            items.push(("Close other tabs".into(), MenuAction::CloseTabs(others)));
        }
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
        if let MenuAction::ShowMoveTargets(tabs) = &action {
            let items = self
                .model
                .destination_groups(tabs)
                .into_iter()
                .map(|group| {
                    (
                        self.group_label(group),
                        MenuAction::MoveToGroup(tabs.clone(), group),
                    )
                })
                .collect();
            if let Some(menu) = &mut self.menu {
                menu.items = items;
            }
            cx.notify();
            return;
        }
        if matches!(action, MenuAction::Separator | MenuAction::TabColors(_)) {
            return;
        }
        self.dismiss_menu(cx);
        match action {
            MenuAction::Rename(target) => return self.start_rename(target, window, cx),
            MenuAction::NewGroup(tabs) => {
                self.model.new_group(&tabs);
            }
            MenuAction::ShowMoveTargets(_) => {}
            MenuAction::MoveToGroup(tabs, group) => self.model.move_to_group(&tabs, group),
            MenuAction::RemoveFromGroup(tabs) => self.model.remove_from_group(&tabs),
            MenuAction::PinTab(index, pinned) => self.model.set_tab_pinned(index, pinned),
            MenuAction::PinGroup(group, pinned) => self.model.set_group_pinned(group, pinned),
            MenuAction::MoveGroup(group, up) => self.model.move_group(group, up),
            MenuAction::CloseTabs(tabs) => self.close_tabs(&tabs, window, cx),
            MenuAction::NewTabInGroup(group) => self.new_tab_in_group(group, window, cx),
            MenuAction::Ungroup(group) => {
                let tabs = self.model.group_tabs(group);
                self.model.remove_from_group(&tabs);
            }
            MenuAction::CloseGroup(group) => self.close_group(group, window, cx),
            MenuAction::ToggleGroup(group) => self.model.toggle_group(group),
            MenuAction::ToggleMaximize => self.model.toggle_maximized(),
            MenuAction::Split(axis) => return self.split(axis, window, cx),
            MenuAction::CopyText(text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text))
            }
            MenuAction::MoveTab(index, up) => self.model.move_tab(index, up),
            MenuAction::SetTabColor(index, color) => self.model.set_tab_color(index, color),
            MenuAction::Separator | MenuAction::TabColors(_) => {}
        }
        self.sync(window, cx);
    }

    /// The pane header's overflow menu, opened below its button like Warp's.
    fn open_pane_menu(
        &mut self,
        id: PaneId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.focus(id) {
            self.sync(window, cx);
        }
        let maximize = if self.model.active().maximized {
            "Minimize pane\t⇧⌘↵"
        } else {
            "Maximize pane\t⇧⌘↵"
        };
        let items = vec![
            (
                "Split pane right\t⌘D".into(),
                MenuAction::Split(Axis::Horizontal),
            ),
            (
                "Split pane down\t⇧⌘D".into(),
                MenuAction::Split(Axis::Vertical),
            ),
            (maximize.into(), MenuAction::ToggleMaximize),
        ];
        let below = gpui::point(position.x - px(MENU_WIDTH - 12.0), position.y + px(12.0));
        self.open_menu(below, items, cx);
    }

    fn render_menu(&self, menu: &ContextMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let items = menu
            .items
            .iter()
            .enumerate()
            .map(|(index, (label, action))| match action {
                MenuAction::Separator => div()
                    .h(px(1.0))
                    .mx_2()
                    .my_1()
                    .bg(hsla(theme.border))
                    .into_any_element(),
                MenuAction::TabColors(tab) => self.render_color_row(*tab, cx),
                action => {
                    let action = action.clone();
                    let (label, shortcut) = label.split_once('\t').unwrap_or((label, ""));
                    div()
                        .id(("menu-item", index))
                        .flex()
                        .gap_2()
                        .px_2()
                        .py(px(5.0))
                        .mx_1()
                        .rounded_sm()
                        .hover(|style| style.bg(hsla(theme.accent)).text_color(hsla(theme.chrome)))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.run_menu_action(action.clone(), window, cx)
                        }))
                        .child(div().flex_1().min_w_0().truncate().child(label.to_owned()))
                        .child(div().opacity(0.6).child(shortcut.to_owned()))
                        .into_any_element()
                }
            })
            .collect::<Vec<_>>();
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
                    .on_mouse_down_out(cx.listener(|view, _, _, cx| view.dismiss_menu(cx)))
                    .children(items),
            ),
        )
        .with_priority(1)
    }

    /// "No color" then each color; the tab's current choice is ringed.
    fn render_color_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let current = self.model.tabs.get(index).and_then(|tab| tab.color);
        let swatch = |slot: usize, color: Option<usize>| {
            div()
                .id(("tab-color", slot))
                .size(px(18.0))
                .rounded_full()
                .border_2()
                .border_color(hsla(if current == color {
                    theme.text
                } else {
                    theme.tab_active
                }))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.run_menu_action(MenuAction::SetTabColor(index, color), window, cx)
                }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px_2()
            .py_1()
            .child(
                swatch(0, None).child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.0))
                        .text_color(hsla(theme.text_muted))
                        .child("⊘"),
                ),
            )
            .children(
                theme
                    .group_colors
                    .iter()
                    .enumerate()
                    .map(|(color, &value)| {
                        swatch(color + 1, Some(color))
                            .child(div().size_full().rounded_full().bg(hsla(value)))
                    }),
            )
            .into_any_element()
    }

    /// A custom title wins over the shell's, as in Warp.
    fn tab_title(&self, tab: &Tab, cx: &Context<Self>) -> String {
        tab.title.clone().unwrap_or_else(|| {
            self.panes
                .get(&tab.active_pane)
                .map_or_else(String::new, |(pane, _)| pane.read(cx).title())
        })
    }

    /// The second line of a tab row: the `~` path, unless the title already shows it.
    fn tab_subtitle(&self, tab: &Tab, title: &str, cx: &Context<Self>) -> Option<String> {
        let (pane, _) = self.panes.get(&tab.active_pane)?;
        Some(pane.read(cx).display_cwd()).filter(|path| path != title)
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let mut rows = Vec::new();
        if let Some(matches) = self.search_matches(cx) {
            // A filtered list is flat: groups and the pin divider would only get in the way.
            if matches.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .py_2()
                        .text_color(hsla(theme.text_muted))
                        .child("No matching tabs")
                        .into_any_element(),
                );
            }
            for index in matches {
                rows.push(self.render_tab_row(index, &self.model.tabs[index], cx));
            }
        } else {
            let mut previous = None;
            for (index, tab) in self.model.tabs.iter().enumerate() {
                if index > 0 && self.model.tab_pinned(index - 1) && !self.model.tab_pinned(index) {
                    rows.push(
                        div()
                            .flex_shrink_0()
                            .h(px(1.0))
                            .bg(hsla(theme.text_muted).opacity(0.5))
                            .into_any_element(),
                    );
                }
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
        }
        let end = self.model.tabs.len();
        let search = self.search.as_ref().map(|(input, _)| input.clone());
        let searching = search
            .as_ref()
            .is_some_and(|input| !input.read(cx).text().is_empty());
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
                    .gap_2()
                    .h(px(40.0))
                    .pl_3()
                    .pr_2()
                    .border_b_1()
                    .border_color(hsla(theme.border))
                    .child(
                        gpui::svg()
                            .path("icons/search.svg")
                            .flex_shrink_0()
                            .size(px(14.0))
                            .text_color(hsla(theme.text_muted)),
                    )
                    .child(
                        div()
                            .id("tab-search")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .cursor_text()
                            .text_color(hsla(theme.text))
                            .on_click(
                                cx.listener(|view, _, window, cx| view.start_search(window, cx)),
                            )
                            .when(!searching, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .text_color(hsla(theme.text_muted))
                                        .child("Search tabs..."),
                                )
                            })
                            .children(search),
                    )
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
                    .children(rows)
                    // Dropping below the list moves a tab to the end, outside any group.
                    .child(
                        div()
                            .id("tab-list-end")
                            .flex_1()
                            .min_h(px(32.0))
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
            .min_h(px(34.0))
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
            .child(match self.rename_field(RenameTarget::Group(id)) {
                Some(field) => field,
                None => div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.group_label(id)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(hsla(theme.text_muted))
                            .child(match tabs.len() {
                                1 => "1 tab".to_owned(),
                                count => format!("{count} tabs"),
                            }),
                    )
                    .into_any_element(),
            })
            .when(group.pinned, |this| this.child(pin_mark(theme)))
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

    /// A Warp-style session card: terminal icon, title and path, separated by lines, with
    /// a menu and close button floating on hover.
    fn render_tab_row(&self, index: usize, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let active = index == self.model.active_tab;
        let title = self.tab_title(tab, cx);
        let subtitle = self.tab_subtitle(tab, &title, cx);
        // Selection highlight only matters once more than one tab is involved.
        let selection = self.model.selected_indices();
        let selected = selection.len() > 1 && selection.contains(&index);
        let group = tab.group;
        let group_color = group
            .and_then(|group| self.model.group(group))
            .map(|group| hsla(theme.group_colors[group.color]));
        let tint = tab.color.map(|color| hsla(theme.group_colors[color]));
        let hover = format!("tab-{index}");
        let dragged = DraggedTab {
            title: if selected {
                format!("{} tabs", selection.len())
            } else {
                title.clone()
            },
            indices: if selected {
                selection.clone()
            } else {
                vec![index]
            },
            background: theme.tab_active,
            text: theme.text,
        };
        let card = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .text_color(hsla(if active || selected {
                theme.text
            } else {
                theme.text_muted
            }))
            .map(|this| match (tint, active) {
                (Some(tint), _) => this.bg(tint.opacity(if active { 0.3 } else { 0.15 })),
                (None, true) => this.bg(hsla(theme.tab_active)),
                (None, false) => {
                    this.group_hover(hover.clone(), |style| style.bg(hsla(theme.tab_hover)))
                }
            })
            .when(active, |this| {
                this.border_color(hsla(theme.text).opacity(0.2))
            })
            .when(selected, |this| {
                this.bg(hsla(theme.accent).opacity(0.22))
                    .border_color(hsla(theme.accent).opacity(0.5))
            })
            .when_some(group_color, |this, color| {
                this.child(div().w(px(3.0)).h(px(26.0)).rounded_sm().bg(color))
            })
            .child(terminal_icon(theme, active, tint))
            .child(match self.rename_field(RenameTarget::Tab(tab.id)) {
                Some(field) => field,
                None => div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(px(13.0)).child(title))
                    .when_some(subtitle, |this, subtitle| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(hsla(theme.text_muted))
                                .child(subtitle),
                        )
                    })
                    .into_any_element(),
            })
            .when(group.is_none() && tab.pinned, |this| {
                this.child(pin_mark(theme))
            });
        div()
            .id(("tab", index))
            .group(hover.clone())
            .relative()
            .flex_shrink_0()
            .p(px(6.0))
            .when(group.is_some(), |this| this.pl(px(14.0)))
            .border_b_1()
            .border_color(hsla(theme.border))
            .on_click(
                cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
                    view.click_tab(index, event.modifiers(), event.click_count(), window, cx)
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
            .child(card)
            .child(
                div()
                    .absolute()
                    .top(px(2.0))
                    .right(px(4.0))
                    .flex()
                    .gap(px(2.0))
                    .p(px(2.0))
                    .rounded_md()
                    .bg(hsla(theme.chrome))
                    .border_1()
                    .border_color(hsla(theme.border))
                    .invisible()
                    .group_hover(hover, |style| style.visible())
                    .child(
                        icon_button(("tab-menu", index), "⋮", theme).on_click(cx.listener(
                            move |view, event: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                view.open_tab_menu(index, event.position(), cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(("close-tab", index), "×", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_tab(index, window, cx)
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    /// Shown once a tab is split, as in Warp: the title, a menu and a close button, with a
    /// corner mark on the focused pane.
    fn render_pane_header(&self, id: PaneId, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let active = id == self.model.active_pane();
        let title = self
            .panes
            .get(&id)
            .map_or_else(String::new, |(pane, _)| pane.read(cx).title());
        let key = id.0 as usize;
        div()
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .h(px(28.0))
            .px(px(56.0))
            .border_b_1()
            .border_color(hsla(theme.border))
            .font_family(theme::UI_FONT)
            .text_size(px(12.0))
            .text_color(hsla(if active { theme.text } else { theme.text_muted }))
            .when(active, |this| {
                this.child(
                    gpui::svg()
                        .path("icons/corner.svg")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(10.0))
                        .text_color(hsla(theme.accent)),
                )
            })
            .child(div().min_w_0().truncate().child(title))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right(px(6.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        icon_button(("pane-menu", key), "⋮", theme).on_click(cx.listener(
                            move |view, event: &gpui::ClickEvent, window, cx| {
                                cx.stop_propagation();
                                view.open_pane_menu(id, event.position(), window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(("pane-close", key), "×", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_pane(id, window, cx)
                            },
                        )),
                    ),
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
                let split = self.model.active().layout.panes().len() > 1;
                let pane = self.panes.get(&id).map(|(pane, _)| pane.clone());
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, window, cx| {
                            if view.model.active_pane() != id && view.model.focus(id) {
                                view.sync(window, cx);
                            }
                        }),
                    )
                    .when(split, |this| this.child(self.render_pane_header(id, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            // Breathing room around the text; the surface can't be padded from GPUI.
                            .p(px(6.0))
                            .children(pane),
                    )
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

/// The session icon: a prompt glyph in a rounded square frame, in the tab's color if any.
fn terminal_icon(theme: &Theme, active: bool, tint: Option<Hsla>) -> impl IntoElement {
    let frame = tint.unwrap_or(hsla(theme.text_muted));
    div()
        .flex_none()
        .size(px(28.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(frame.opacity(if active { 0.7 } else { 0.4 }))
        .flex()
        .items_center()
        .justify_center()
        .bg(match tint {
            Some(tint) => tint.opacity(0.18),
            None => hsla(if active {
                theme.border
            } else {
                theme.tab_active
            }),
        })
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(tint.unwrap_or(hsla(theme.text)))
        .child(">_")
}

fn pin_mark(theme: &Theme) -> impl IntoElement {
    gpui::svg()
        .path("icons/pin.svg")
        .flex_shrink_0()
        .size(px(12.0))
        .text_color(hsla(theme.text_muted))
}
