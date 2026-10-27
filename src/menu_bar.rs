//! The macOS menu bar.
use crate::actions::*;
use gpui::{Menu, MenuItem};

/// Warp's menus, without what bareframe doesn't have (blocks, AI, Drive, help).
pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("bareframe").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide bareframe", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit bareframe", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Terminal Tab", NewTab),
            MenuItem::action("New Browser Tab", NewBrowserTab),
            MenuItem::action("Restart Failed Session", RestartSession),
            MenuItem::separator(),
            MenuItem::action("Close Current Session", ClosePane),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy", Copy),
            MenuItem::action("Paste", Paste),
            MenuItem::action("Select All", TerminalSelectAll),
            MenuItem::action("Clear Screen", ClearScreen),
            MenuItem::separator(),
            MenuItem::action("Find in Files", FindInFiles),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Tabs Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Tools Panel", ToggleToolsPanel),
            MenuItem::separator(),
            MenuItem::action("Command Palette", ToggleCommandPalette),
            MenuItem::action("Tools Panel: Project Explorer", ShowProjectExplorer),
            MenuItem::action("Tools Panel: Global Search", ShowGlobalSearch),
            MenuItem::action("Code Review", ToggleCodeReview),
            MenuItem::separator(),
            MenuItem::action("Dark Theme", UseDarkTheme),
            MenuItem::action("Light Theme", UseLightTheme),
            MenuItem::separator(),
            MenuItem::action("Zoom In", IncreaseFontSize),
            MenuItem::action("Zoom Out", DecreaseFontSize),
            MenuItem::action("Reset Zoom", ResetFontSize),
        ]),
        Menu::new("Tab").items([
            MenuItem::action("Rename the Current Tab", RenameTab),
            MenuItem::separator(),
            MenuItem::action("Split Pane Right", SplitRight),
            MenuItem::action("Split Pane Down", SplitDown),
            MenuItem::separator(),
            MenuItem::action("Move Tab Up", MoveTabUp),
            MenuItem::action("Move Tab Down", MoveTabDown),
            MenuItem::action("Add Tab to New Group", GroupTab),
            MenuItem::action("Remove Tab from Group", UngroupTab),
            MenuItem::separator(),
            MenuItem::action("Switch to Next Tab", NextTab),
            MenuItem::action("Switch to Previous Tab", PreviousTab),
            MenuItem::separator(),
            MenuItem::action("Activate Next Pane", FocusNextPane),
            MenuItem::action("Activate Previous Pane", FocusPreviousPane),
            MenuItem::action("Equalize Panes", EqualizePanes),
            MenuItem::separator(),
            MenuItem::action("Toggle Maximize Active Pane", ToggleMaximizePane),
            MenuItem::separator(),
            MenuItem::action("Close the Current Tab", CloseTab),
            MenuItem::action("Close Other Tabs", CloseOtherTabs),
        ]),
        // Named "Window", macOS lists the open windows at its end; it also adds
        // Enter Full Screen to View.
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", ZoomWindow),
        ]),
    ]
}
