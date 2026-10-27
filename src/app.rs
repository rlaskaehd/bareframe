use crate::workspace::WorkspaceView;
use bareframe_core::{
    config::LaunchConfig,
    settings::{Settings, config_path, state_path},
    state::{self, SavedState},
};
use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowOptions, actions, point, px, size,
};
use std::path::{Path, PathBuf};

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
    cx.bind_keys(crate::rename::key_bindings());
    cx.bind_keys(crate::explorer::key_bindings());
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

/// Height of the sidebar header that replaces the system title bar.
pub const TITLE_BAR_HEIGHT: f32 = 38.0;

pub fn is_validation_instance() -> bool {
    cfg!(debug_assertions) && std::env::var("BAREFRAME_VALIDATION").as_deref() == Ok("1")
}

pub fn name() -> &'static str {
    if is_validation_instance() {
        "bareframe validation"
    } else {
        "bareframe"
    }
}

/// Icons compiled into the binary; GPUI renders them as masks in the text color.
struct Assets;

const ICONS: [(&str, &[u8]); 9] = [
    ("icons/pin.svg", include_bytes!("../assets/icons/pin.svg")),
    (
        "icons/corner.svg",
        include_bytes!("../assets/icons/corner.svg"),
    ),
    (
        "icons/search.svg",
        include_bytes!("../assets/icons/search.svg"),
    ),
    ("icons/gear.svg", include_bytes!("../assets/icons/gear.svg")),
    (
        "icons/wrench.svg",
        include_bytes!("../assets/icons/wrench.svg"),
    ),
    (
        "icons/folder.svg",
        include_bytes!("../assets/icons/folder.svg"),
    ),
    ("icons/file.svg", include_bytes!("../assets/icons/file.svg")),
    (
        "icons/chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
];

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| (*bytes).into()))
    }

    fn list(&self, _: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(ICONS.iter().map(|(name, _)| (*name).into()).collect())
    }
}

/// The vendored Ghostty zsh integration: in the app bundle, else in the source tree.
pub fn zsh_integration_dir() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("../Resources/shell-integration/zsh")));
    let source = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty/src/shell-integration/zsh"
    ));
    bundled
        .into_iter()
        .chain([source])
        .find(|dir| dir.join(".zshenv").is_file())
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A file beside the settings; the validation instance keeps its own under `target/` so
/// results are reproducible.
fn app_file(name: &str, path: fn(Option<&Path>, &Path) -> PathBuf) -> Option<PathBuf> {
    if is_validation_instance() {
        return Some(
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target/validation")).join(name),
        );
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME");
    Some(path(xdg.as_deref().map(Path::new), &home_dir()?))
}

pub fn config_file() -> Option<PathBuf> {
    app_file("config", config_path)
}

/// Where the window layout is saved.
pub fn state_file() -> Option<PathBuf> {
    app_file("state", state_path)
}

/// The file's text, or `None` if it does not exist.
pub fn read_optional(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes through a temporary file, so a crash mid-write leaves the previous file intact.
/// A symbolic link (a settings file kept in a dotfiles repository) is followed, so the
/// rename replaces the file it points to rather than the link.
pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let temporary = path.with_extension("tmp");
    path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// The file's text and the settings it holds; a missing file gives the defaults.
pub fn load_settings() -> (Settings, String) {
    let Some(path) = config_file() else {
        return (Settings::default(), String::new());
    };
    let text = read_optional(&path)
        .unwrap_or_else(|error| {
            eprintln!("bareframe: {}: {error}", path.display());
            None
        })
        .unwrap_or_default();
    let (settings, warnings) = Settings::parse(&text);
    for warning in warnings {
        eprintln!("bareframe: {}: {warning}", path.display());
    }
    (settings, text)
}

/// Rewrites one key in the settings file, keeping the user's comments and other lines.
/// Returns the new text so the file watcher can tell this write from an outside edit.
pub fn save_setting(key: &str, value: &str, default: &str) -> Result<String, String> {
    edit_config(|text| bareframe_core::settings::write_value(text, key, value, default))
}

/// Reads the settings file (empty if missing), rewrites it with `change` and writes it back
/// through a temporary file. Returns the new text.
pub fn edit_config(change: impl FnOnce(&str) -> String) -> Result<String, String> {
    let path = config_file().ok_or("no home directory")?;
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    let text = change(&read_optional(&path).map_err(failed)?.unwrap_or_default());
    write_atomically(&path, &text).map_err(failed)?;
    Ok(text)
}

/// A missing file is a first launch; a broken one is reported and left for inspection.
fn load_state() -> Option<SavedState> {
    let path = state_file()?;
    let text = read_optional(&path)
        .inspect_err(|error| eprintln!("bareframe: {}: {error}", path.display()))
        .ok()??;
    state::decode(&text)
        .inspect_err(|error| eprintln!("bareframe: {}: {error}", path.display()))
        .ok()
}

pub fn run(config: LaunchConfig, restore: bool) {
    let (settings, config_text) = load_settings();
    let saved = (restore && settings.flag("restore-state"))
        .then(load_state)
        .flatten();
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.on_action(|_: &Quit, cx| cx.quit());
            bind_keys(&settings, cx);
            cx.set_menus(vec![
                Menu::new("bareframe").items([
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit bareframe", Quit),
                ]),
                Menu::new("Shell").items([
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Split Right", SplitRight),
                    MenuItem::action("Split Down", SplitDown),
                    MenuItem::action("Maximize Pane", ToggleMaximizePane),
                    MenuItem::separator(),
                    MenuItem::action("Add Tab to New Group", GroupTab),
                    MenuItem::action("Remove Tab from Group", UngroupTab),
                    MenuItem::separator(),
                    MenuItem::action("Restart Failed Session", RestartSession),
                    MenuItem::separator(),
                    MenuItem::action("Close Pane", ClosePane),
                    MenuItem::action("Close Window", CloseWindow),
                ]),
                Menu::new("Edit").items([
                    MenuItem::action("Copy", Copy),
                    MenuItem::action("Paste", Paste),
                ]),
                Menu::new("View").items([
                    MenuItem::action("Command Palette", ToggleCommandPalette),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Tabs Sidebar", ToggleSidebar),
                    MenuItem::action("Toggle Tools Panel", ToggleToolsPanel),
                    MenuItem::action("Project Explorer", ShowProjectExplorer),
                    MenuItem::action("Global Search", ShowGlobalSearch),
                    MenuItem::separator(),
                    MenuItem::action("Dark Theme", UseDarkTheme),
                    MenuItem::action("Light Theme", UseLightTheme),
                    MenuItem::separator(),
                    MenuItem::action("Increase Font Size", IncreaseFontSize),
                    MenuItem::action("Decrease Font Size", DecreaseFontSize),
                    MenuItem::action("Reset Font Size", ResetFontSize),
                ]),
                Menu::new("Window").items([
                    MenuItem::action("Next Tab", NextTab),
                    MenuItem::action("Previous Tab", PreviousTab),
                ]),
            ]);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            // A saved place on a display that is gone, or too small to use, opens centered.
            let bounds = saved
                .as_ref()
                .and_then(|saved| saved.window)
                .map(|[x, y, width, height]| {
                    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
                })
                .filter(|bounds| {
                    let title = point(
                        bounds.origin.x + bounds.size.width / 2.0,
                        bounds.origin.y + px(TITLE_BAR_HEIGHT / 2.0),
                    );
                    bounds.size.width >= px(300.0)
                        && bounds.size.height >= px(200.0)
                        && cx
                            .displays()
                            .iter()
                            .any(|display| display.bounds().contains(&title))
                })
                .unwrap_or_else(|| Bounds::centered(None, size(px(1040.0), px(720.0)), cx));
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(name().into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(14.0), px(13.0))),
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| {
                        WorkspaceView::new(config, settings, config_text, saved, window, cx)
                    });
                    let weak = view.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        if let Err(error) = weak.update(cx, |view, cx| view.shutdown(cx)) {
                            eprintln!("bareframe: closing window: {error}");
                        }
                        true
                    });
                    view
                },
            ) {
                eprintln!("bareframe: could not open window: {error}");
                cx.quit();
            }
            cx.activate(true);
        });
}
