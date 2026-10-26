use crate::workspace::WorkspaceView;
use bareframe_core::config::LaunchConfig;
use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, WindowBounds, WindowOptions, actions, px,
    size,
};

actions!(bareframe, [Quit, CloseWindow, Copy, Paste, RestartSession]);

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

pub fn run(config: LaunchConfig) {
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-c", Copy, Some("BareframeTerminal")),
            KeyBinding::new("cmd-v", Paste, Some("BareframeTerminal")),
            KeyBinding::new("cmd-shift-r", RestartSession, None),
        ]);
        cx.set_menus(vec![
            Menu::new("bareframe").items([MenuItem::action("Quit bareframe", Quit)]),
            Menu::new("Shell").items([
                MenuItem::action("Restart Exited Session", RestartSession),
                MenuItem::action("Close Window", CloseWindow),
            ]),
            Menu::new("Edit").items([
                MenuItem::action("Copy", Copy),
                MenuItem::action("Paste", Paste),
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
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title(name());
                let view = cx.new(|cx| WorkspaceView::new(config, window, cx));
                let weak = view.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    let _ = weak.update(cx, |view, cx| view.shutdown(cx));
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
