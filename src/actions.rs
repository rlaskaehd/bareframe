//! Every bareframe action, the keymap names that reach them, and installing key bindings.
use bareframe_core::settings::Settings;
use gpui::{App, KeyBinding, actions};

actions!(
    bareframe,
    [
        Quit,
        CloseWindow,
        ClosePane,
        Copy,
        Paste,
        RestartSession,
        NewTab,
        NextTab,
        PreviousTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectLastTab,
        SplitRight,
        SplitDown,
        ToggleMaximizePane,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        UseDarkTheme,
        UseLightTheme,
        GroupTab,
        UngroupTab,
        ToggleSidebar,
        OpenSettings,
        ToggleToolsPanel,
        ShowProjectExplorer,
        ShowGlobalSearch,
        FindInFiles,
        CloseTab,
        FocusPreviousPane,
        FocusNextPane,
        EqualizePanes,
        ReloadSettings,
        ToggleFullscreen,
        TerminalSelectAll,
        ClearScreen,
        ScrollToTop,
        ScrollToBottom,
        ScrollPageUp,
        ScrollPageDown,
        JumpToPreviousPrompt,
        JumpToNextPrompt,
        ToggleCommandPalette,
        ToggleCodeReview,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        ZoomWindow,
        RenameTab,
        MoveTabUp,
        MoveTabDown,
        CloseOtherTabs,
    ]
);

/// The GPUI action for a keymap action name (see `bareframe_core::keymap::ACTIONS`).
pub fn keymap_action(name: &str) -> Option<Box<dyn gpui::Action>> {
    Some(match name {
        "new_tab" => Box::new(NewTab),
        "close_surface" => Box::new(ClosePane),
        "close_tab" => Box::new(CloseTab),
        "close_window" => Box::new(CloseWindow),
        "quit" => Box::new(Quit),
        "new_split:right" => Box::new(SplitRight),
        "new_split:down" => Box::new(SplitDown),
        "goto_split:previous" => Box::new(FocusPreviousPane),
        "goto_split:next" => Box::new(FocusNextPane),
        "goto_split:left" => Box::new(FocusLeft),
        "goto_split:right" => Box::new(FocusRight),
        "goto_split:up" => Box::new(FocusUp),
        "goto_split:down" => Box::new(FocusDown),
        "equalize_splits" => Box::new(EqualizePanes),
        "toggle_split_zoom" => Box::new(ToggleMaximizePane),
        "previous_tab" => Box::new(PreviousTab),
        "next_tab" => Box::new(NextTab),
        "goto_tab:1" => Box::new(SelectTab1),
        "goto_tab:2" => Box::new(SelectTab2),
        "goto_tab:3" => Box::new(SelectTab3),
        "goto_tab:4" => Box::new(SelectTab4),
        "goto_tab:5" => Box::new(SelectTab5),
        "goto_tab:6" => Box::new(SelectTab6),
        "goto_tab:7" => Box::new(SelectTab7),
        "goto_tab:8" => Box::new(SelectTab8),
        "last_tab" => Box::new(SelectLastTab),
        "copy_to_clipboard" => Box::new(Copy),
        "paste_from_clipboard" => Box::new(Paste),
        "select_all" => Box::new(TerminalSelectAll),
        "clear_screen" => Box::new(ClearScreen),
        "scroll_to_top" => Box::new(ScrollToTop),
        "scroll_to_bottom" => Box::new(ScrollToBottom),
        "scroll_page_up" => Box::new(ScrollPageUp),
        "scroll_page_down" => Box::new(ScrollPageDown),
        "jump_to_prompt:-1" => Box::new(JumpToPreviousPrompt),
        "jump_to_prompt:1" => Box::new(JumpToNextPrompt),
        "increase_font_size:1" => Box::new(IncreaseFontSize),
        "decrease_font_size:1" => Box::new(DecreaseFontSize),
        "reset_font_size" => Box::new(ResetFontSize),
        "toggle_command_palette" => Box::new(ToggleCommandPalette),
        "toggle_code_review" => Box::new(ToggleCodeReview),
        "open_config" => Box::new(OpenSettings),
        "reload_config" => Box::new(ReloadSettings),
        "toggle_fullscreen" => Box::new(ToggleFullscreen),
        "restart_session" => Box::new(RestartSession),
        "toggle_tab_sidebar" => Box::new(ToggleSidebar),
        "toggle_tools_panel" => Box::new(ToggleToolsPanel),
        "show_project_explorer" => Box::new(ShowProjectExplorer),
        "show_global_search" => Box::new(ShowGlobalSearch),
        "find_in_files" => Box::new(FindInFiles),
        "group_tab" => Box::new(GroupTab),
        "ungroup_tab" => Box::new(UngroupTab),
        _ => return None,
    })
}

/// Replaces every key binding with the defaults plus the settings file's `keybind` lines.
pub fn bind_keys(settings: &Settings, cx: &mut App) {
    use bareframe_core::keymap::{self, Scope};
    cx.clear_key_bindings();
    // macOS's own app and window keys, which the keymap can still rebind.
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
    ]);
    cx.bind_keys(crate::text_field::key_bindings());
    cx.bind_keys(crate::explorer::key_bindings());
    cx.bind_keys(crate::editor::key_bindings());
    cx.bind_keys(crate::markdown::key_bindings());
    let (bindings, warnings) = keymap::resolve(&settings.keybinds);
    for warning in warnings {
        eprintln!("bareframe: settings: {warning}");
    }
    let terminal = gpui::KeyBindingContextPredicate::parse("BareframeTerminal")
        .ok()
        .map(std::rc::Rc::new);
    let mut loaded = Vec::new();
    for (trigger, name) in bindings {
        let (Some(action), Some(def)) = (keymap_action(name), keymap::find(name)) else {
            eprintln!("bareframe: no handler for keymap action {name}");
            continue;
        };
        let context = (def.scope == Scope::Terminal)
            .then(|| terminal.clone())
            .flatten();
        match KeyBinding::load(
            &trigger.to_gpui(),
            action,
            context,
            false,
            None,
            &gpui::DummyKeyboardMapper,
        ) {
            Ok(binding) => loaded.push(binding),
            Err(error) => eprintln!("bareframe: keybind {trigger}: {error:?}"),
        }
    }
    cx.bind_keys(loaded);
}
