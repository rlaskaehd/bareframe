//! Chromium's callbacks for one browser, turned into events its pane reads. They run on
//! the main thread, inside the message pump, and only send, store or remove: a panic here
//! would abort the app.
use super::{engine, mac};
use cef::*;
use futures::channel::mpsc::UnboundedSender;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub enum BrowserEvent {
    Address(String),
    Title(String),
    Loading {
        loading: bool,
        back: bool,
        forward: bool,
    },
    /// The user asked the page for a new window (a link with `target=_blank`, say); it
    /// opens as a new browser tab.
    Popup(String),
    /// The page took the keyboard, from a click on it.
    Focused,
    /// The page closed itself (`window.close()`); the browser is gone.
    Closed,
}

/// Set while the pane has GPUI's focus on its page, so the page may take the keyboard.
pub type Keyboard = Arc<AtomicBool>;

thread_local! {
    /// Browsers an agent just right-clicked, and when (see `quiet_menu`).
    static AGENT_MENUS: RefCell<HashMap<i32, Instant>> = RefCell::default();
}

/// An agent is about to right-click `browser`: the page gets its `contextmenu`, but the
/// menu Chromium would show next, which no agent can use, stays shut. A page that shows
/// its own menu leaves nothing behind past a few seconds for the user's right-clicks.
pub fn quiet_menu(browser: i32) {
    AGENT_MENUS.with_borrow_mut(|menus| menus.insert(browser, Instant::now()));
}

pub fn client(events: UnboundedSender<BrowserEvent>, keyboard: Keyboard) -> Client {
    PaneClient::new(events, keyboard)
}

wrap_client! {
    struct PaneClient {
        events: UnboundedSender<BrowserEvent>,
        keyboard: Keyboard,
    }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(PaneDisplay::new(self.events.clone()))
        }

        fn focus_handler(&self) -> Option<FocusHandler> {
            Some(PaneFocus::new(self.events.clone(), self.keyboard.clone()))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(PaneLifeSpan::new(self.events.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(PaneLoad::new(self.events.clone()))
        }

        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(PaneMenu::new())
        }
    }
}

wrap_context_menu_handler! {
    struct PaneMenu;

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            let (Some(browser), Some(model)) = (browser, model) else {
                return;
            };
            let id = browser.identifier();
            let quiet = AGENT_MENUS
                .try_with(|menus| {
                    let at = menus.try_borrow_mut().ok()?.remove(&id)?;
                    Some(at.elapsed() < Duration::from_secs(3))
                })
                .ok()
                .flatten()
                .unwrap_or(false);
            // An empty menu is not shown.
            if quiet {
                model.clear();
            }
        }
    }
}

wrap_display_handler! {
    struct PaneDisplay {
        events: UnboundedSender<BrowserEvent>,
    }

    impl DisplayHandler {
        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            // Only the page itself, not frames inside it, moves the address bar.
            if frame.is_some_and(|frame| frame.is_main() != 0) {
                let url = url.map(CefString::to_string).unwrap_or_default();
                let _ = self.events.unbounded_send(BrowserEvent::Address(url));
            }
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let title = title.map(CefString::to_string).unwrap_or_default();
            let _ = self.events.unbounded_send(BrowserEvent::Title(title));
        }
    }
}

wrap_focus_handler! {
    struct PaneFocus {
        events: UnboundedSender<BrowserEvent>,
        keyboard: Keyboard,
    }

    impl FocusHandler {
        fn on_set_focus(
            &self,
            browser: Option<&mut Browser>,
            _source: FocusSource,
        ) -> ::std::os::raw::c_int {
            // The page gets the keyboard when its pane is focused or the user clicks it, never
            // by itself (a load, or a script): keys typed in the terminal beside it stay there.
            let view = browser.and_then(|browser| browser.host()).map(|host| host.window_handle());
            (!self.keyboard.load(Ordering::Relaxed) && !view.is_some_and(mac::clicking)).into()
        }

        fn on_got_focus(&self, _browser: Option<&mut Browser>) {
            let _ = self.events.unbounded_send(BrowserEvent::Focused);
        }
    }
}

wrap_life_span_handler! {
    struct PaneLifeSpan {
        events: UnboundedSender<BrowserEvent>,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            // Only a click on the page itself counts; scripts and frames inside it (ads)
            // opening windows on their own are dropped.
            let main = frame.is_some_and(|frame| frame.is_main() != 0);
            if main && user_gesture != 0
                && let Some(url) = target_url.map(CefString::to_string).filter(|url| !url.is_empty())
            {
                let _ = self.events.unbounded_send(BrowserEvent::Popup(url));
            }
            // Cancels Chromium's own window.
            1
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            // Left to itself, Chromium closes the window the page sits in: the whole GPUI
            // window. Taking the page's view out instead ends the browser alone.
            if let Some(browser) = browser {
                engine::closing(browser.identifier());
                if let Some(host) = browser.host() {
                    mac::remove(host.window_handle());
                }
            }
            let _ = self.events.unbounded_send(BrowserEvent::Closed);
            1
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            engine::closed();
        }
    }
}

wrap_load_handler! {
    struct PaneLoad {
        events: UnboundedSender<BrowserEvent>,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            can_go_back: ::std::os::raw::c_int,
            can_go_forward: ::std::os::raw::c_int,
        ) {
            let _ = self.events.unbounded_send(BrowserEvent::Loading {
                loading: is_loading != 0,
                back: can_go_back != 0,
                forward: can_go_forward != 0,
            });
        }
    }
}
