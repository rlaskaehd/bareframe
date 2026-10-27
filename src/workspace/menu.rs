//! Sidebar and pane context menus.
use super::{RenameTarget, WorkspaceView};
use crate::theme::{self, hsla};
use bareframe_core::workspace::{Axis, GroupId, PaneId};
use gpui::{AnyElement, Context, Pixels, Point, Window, anchored, deferred, div, prelude::*, px};

/// Width of every context menu. Menus may overlap the terminals because `open_menu`
/// swaps the native surfaces for snapshots while one is open.
const MENU_WIDTH: f32 = 188.0;

#[derive(Clone)]
pub(super) enum MenuAction {
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
    OpenPath(std::path::PathBuf),
    Reveal(std::path::PathBuf),
    CdTo(std::path::PathBuf),
    NewTabAt(std::path::PathBuf),
    /// Rendered as a divider line.
    Separator,
    /// Rendered as the row of color dots for this tab.
    TabColors(usize),
}

pub(super) struct ContextMenu {
    position: Point<Pixels>,
    /// A tab in a label separates the right-aligned shortcut.
    items: Vec<(String, MenuAction)>,
}

impl WorkspaceView {
    pub(super) fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        if self.frozen && self.clipboard_prompt.is_none() {
            self.set_frozen(false, cx);
        }
        cx.notify();
    }

    pub(super) fn open_menu(
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

    /// Right-clicking a tab in a multi-selection acts on the selection; any other
    /// tab clears the selection and gets its own menu. Sections follow Warp's order.
    pub(super) fn open_tab_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
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
            if let Some(pane) = self.terminal(pane) {
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

    pub(super) fn open_group_menu(
        &mut self,
        id: GroupId,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn run_menu_action(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            MenuAction::CloseGroup(group) => {
                self.close_tabs(&self.model.group_tabs(group), window, cx)
            }
            MenuAction::ToggleGroup(group) => self.model.toggle_group(group),
            MenuAction::ToggleMaximize => self.model.toggle_maximized(),
            MenuAction::Split(axis) => return self.split(axis, window, cx),
            MenuAction::CopyText(text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text))
            }
            MenuAction::MoveTab(index, up) => self.model.move_tab(index, up),
            MenuAction::OpenPath(path) => self.open_file(&path, window, cx),
            MenuAction::Reveal(path) => super::tools::reveal_in_finder(&path),
            MenuAction::CdTo(path) => return self.cd_to(&path, window, cx),
            MenuAction::NewTabAt(path) => return self.new_tab_at(path, window, cx),
            MenuAction::SetTabColor(index, color) => self.model.set_tab_color(index, color),
            MenuAction::Separator | MenuAction::TabColors(_) => {}
        }
        self.sync(window, cx);
    }

    /// The pane header's overflow menu, opened below its button like Warp's.
    pub(super) fn open_pane_menu(
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

    pub(super) fn render_menu(
        &self,
        menu: &ContextMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                    .on_mouse_down_out(cx.listener(|view, _, window, cx| view.sync(window, cx)))
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
}
