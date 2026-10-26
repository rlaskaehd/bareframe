use crate::workspace::WorkspaceView;
use bareframe_core::{
    config::LaunchConfig,
    settings::{Settings, config_path},
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
    ]
);

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

const ICONS: [(&str, &[u8]); 3] = [
    ("icons/pin.svg", include_bytes!("../assets/icons/pin.svg")),
    (
        "icons/corner.svg",
        include_bytes!("../assets/icons/corner.svg"),
    ),
    (
        "icons/search.svg",
        include_bytes!("../assets/icons/search.svg"),
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

/// The validation instance ignores the user's file so results are reproducible.
fn load_settings() -> Settings {
    if is_validation_instance() {
        return Settings::default();
    }
    let Some(home) = home_dir() else {
        return Settings::default();
    };
    let xdg = std::env::var_os("XDG_CONFIG_HOME");
    let path = config_path(xdg.as_deref().map(Path::new), &home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(error) => {
            eprintln!("bareframe: {}: {error}", path.display());
            return Settings::default();
        }
    };
    let (settings, warnings) = Settings::parse(&text);
    for warning in warnings {
        eprintln!("bareframe: {}: {warning}", path.display());
    }
    settings
}

pub fn run(config: LaunchConfig) {
    let settings = load_settings();
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.bind_keys(crate::rename::key_bindings());
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-shift-w", CloseWindow, None),
                KeyBinding::new("cmd-w", ClosePane, None),
                KeyBinding::new("cmd-c", Copy, Some("BareframeTerminal")),
                KeyBinding::new("cmd-v", Paste, Some("BareframeTerminal")),
                KeyBinding::new("cmd-shift-r", RestartSession, None),
                KeyBinding::new("cmd-t", NewTab, None),
                KeyBinding::new("cmd-}", NextTab, None),
                KeyBinding::new("cmd-{", PreviousTab, None),
                KeyBinding::new("cmd-1", SelectTab1, None),
                KeyBinding::new("cmd-2", SelectTab2, None),
                KeyBinding::new("cmd-3", SelectTab3, None),
                KeyBinding::new("cmd-4", SelectTab4, None),
                KeyBinding::new("cmd-5", SelectTab5, None),
                KeyBinding::new("cmd-6", SelectTab6, None),
                KeyBinding::new("cmd-7", SelectTab7, None),
                KeyBinding::new("cmd-8", SelectTab8, None),
                KeyBinding::new("cmd-9", SelectLastTab, None),
                KeyBinding::new("cmd-d", SplitRight, None),
                KeyBinding::new("cmd-shift-d", SplitDown, None),
                KeyBinding::new("cmd-shift-enter", ToggleMaximizePane, None),
                KeyBinding::new("cmd-alt-left", FocusLeft, None),
                KeyBinding::new("cmd-alt-right", FocusRight, None),
                KeyBinding::new("cmd-alt-up", FocusUp, None),
                KeyBinding::new("cmd-alt-down", FocusDown, None),
                KeyBinding::new("cmd-=", IncreaseFontSize, None),
                KeyBinding::new("cmd-+", IncreaseFontSize, None),
                KeyBinding::new("cmd--", DecreaseFontSize, None),
                KeyBinding::new("cmd-0", ResetFontSize, None),
                KeyBinding::new("cmd-g", GroupTab, None),
                KeyBinding::new("cmd-shift-g", UngroupTab, None),
                // Warp's tabs panel key, plus the earlier Cmd+Shift+B.
                KeyBinding::new("cmd-b", ToggleSidebar, None),
                KeyBinding::new("cmd-shift-b", ToggleSidebar, None),
            ]);
            cx.set_menus(vec![
                Menu::new("bareframe").items([MenuItem::action("Quit bareframe", Quit)]),
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
                    MenuItem::action("Toggle Tabs Sidebar", ToggleSidebar),
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
            let bounds = Bounds::centered(None, size(px(1040.0), px(720.0)), cx);
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
                    let view = cx.new(|cx| WorkspaceView::new(config, settings, window, cx));
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
