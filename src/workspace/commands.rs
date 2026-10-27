//! The window's actions, from menus, shortcuts and the command palette, carried out on the
//! workspace.
use super::{Exit, PaneView, RenameTarget, WorkspaceView, menu::MenuAction, tools};
use crate::actions::*;
use bareframe_core::{
    layout::{Axis, Direction},
    settings::ThemeName,
};
use gpui::{Context, prelude::*};

impl WorkspaceView {
    /// `root` handling every window action that acts on the workspace.
    pub(super) fn with_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|view, _: &ToggleSidebar, _, cx| view.toggle_sidebar(cx)))
            .on_action(
                cx.listener(|view, _: &ToggleCodeReview, window, cx| {
                    view.toggle_review(window, cx)
                }),
            )
            .on_action(cx.listener(|view, _: &ToggleCommandPalette, window, cx| {
                view.toggle_palette(window, cx)
            }))
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
            .on_action(cx.listener(|view, _: &CloseTab, window, cx| {
                let tab = view.model.active_tab;
                view.close_tabs(&[tab], window, cx)
            }))
            .on_action(cx.listener(|view, _: &FocusPreviousPane, window, cx| {
                if view.model.focus_cycle(false) {
                    view.sync(window, cx)
                }
            }))
            .on_action(cx.listener(|view, _: &FocusNextPane, window, cx| {
                if view.model.focus_cycle(true) {
                    view.sync(window, cx)
                }
            }))
            .on_action(cx.listener(|view, _: &EqualizePanes, window, cx| {
                let tab = view.model.active_tab;
                view.model.tabs[tab].layout.equalize();
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| window.toggle_fullscreen()))
            .on_action(cx.listener(|view, _: &ReloadSettings, _, cx| view.reload_from_disk(cx)))
            .on_action(cx.listener(|view, _: &CloseWindow, window, cx| {
                view.exit(Exit::CloseWindow, window, cx)
            }))
            .on_action(cx.listener(|view, _: &Quit, window, cx| view.exit(Exit::Quit, window, cx)))
            .on_action(cx.listener(|view, _: &ClosePane, window, cx| {
                let pane = view.model.active_pane();
                // With several files open, Cmd+W closes the shown file, as editors do.
                if let Some(PaneView::Editor(tabs)) = view.pane(pane).cloned()
                    && tabs.read(cx).editors().count() > 1
                {
                    return tabs.update(cx, |tabs, cx| tabs.close_active(cx));
                }
                view.close_pane(pane, window, cx)
            }))
            .on_action(cx.listener(|view, _: &RestartSession, window, cx| {
                let theme = view.terminal_theme();
                if let Some(pane) = view.terminal(view.model.active_pane()) {
                    pane.update(cx, |pane, cx| pane.retry(&theme, window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &NewTab, window, cx| view.new_tab(window, cx)))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
            .on_action(cx.listener(|view, _: &RenameTab, window, cx| {
                // The name is edited in place on the tab's card.
                view.sidebar_open = true;
                view.reveal_active_tab(cx);
                let id = view.model.active().id;
                view.start_rename(RenameTarget::Tab(id), window, cx)
            }))
            .on_action(cx.listener(|view, _: &MoveTabUp, window, cx| {
                let index = view.model.active_tab;
                view.run_menu_action(MenuAction::MoveTab(index, true), window, cx)
            }))
            .on_action(cx.listener(|view, _: &MoveTabDown, window, cx| {
                let index = view.model.active_tab;
                view.run_menu_action(MenuAction::MoveTab(index, false), window, cx)
            }))
            .on_action(cx.listener(|view, _: &CloseOtherTabs, window, cx| {
                let active = view.model.active_tab;
                let others = (0..view.model.tabs.len())
                    .filter(|&i| i != active)
                    .collect();
                view.run_menu_action(MenuAction::CloseTabs(others), window, cx)
            }))
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
    }
}
