//! Starting bareframe: the application, its app-level actions and the window.
use crate::{
    actions::{Hide, HideOthers, Quit, ShowAll, bind_keys},
    files,
    workspace::WorkspaceView,
};
use bareframe_core::config::LaunchConfig;
use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

/// Height of the sidebar header that replaces the system title bar.
pub const TITLE_BAR_HEIGHT: f32 = 38.0;

pub fn is_validation_instance() -> bool {
    cfg!(debug_assertions) && std::env::var("BAREFRAME_VALIDATION").as_deref() == Ok("1")
}

pub fn name() -> &'static str {
    if is_validation_instance() {
        "bareframe validation"
    } else if cfg!(debug_assertions) {
        "bareframe dev"
    } else {
        "bareframe"
    }
}

pub fn run(config: LaunchConfig, restore: bool) {
    let (settings, config_text) = files::load_settings();
    let saved = (restore && settings.flag("restore-state"))
        .then(files::load_state)
        .flatten();
    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx: &mut App| {
            cx.on_action(|_: &Quit, cx| crate::browser::engine::quit(cx));
            bind_keys(&settings, cx);
            cx.on_action(|_: &Hide, cx| cx.hide());
            cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
            cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
            cx.set_menus(crate::menu_bar::menus());
            crate::lsp::init(cx);
            crate::browser::engine::warm_up();
            crate::update::init(cx);
            cx.on_action(|_: &crate::actions::CheckForUpdates, cx| {
                crate::update::global(cx).update(cx, |updates, cx| updates.check(true, cx))
            });
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    crate::browser::engine::quit(cx);
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
                    window.on_window_should_close(cx, move |window, cx| {
                        // Unsaved editor changes keep the window open until answered.
                        let closing = weak.update(cx, |view, cx| {
                            let closing = view.confirm_unsaved(window, cx);
                            if closing {
                                view.shutdown(cx);
                            } else {
                                // Closes the window once the questions are answered.
                                view.wait_to_close();
                            }
                            closing
                        });
                        closing.unwrap_or_else(|error| {
                            eprintln!("bareframe: closing window: {error}");
                            true
                        })
                    });
                    view
                },
            ) {
                eprintln!("bareframe: could not open window: {error}");
                cx.quit();
            }
            // A validation run started with BAREFRAME_BACKGROUND stays behind the user's
            // windows, so checking a build doesn't take their keyboard.
            if !(is_validation_instance() && std::env::var_os("BAREFRAME_BACKGROUND").is_some()) {
                cx.activate(true);
            }
        });
}
