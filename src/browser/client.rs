//! Chromium's callbacks for one browser, turned into events its pane reads. They run on
//! the main thread, inside the message pump, and only send, store or remove: a panic here
//! would abort the app.
use super::{engine, mac};
use cef::*;
use futures::channel::mpsc::UnboundedSender;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
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
    /// Downloads under way, by id, and where each goes: Chromium writes a temporary file
    /// until the end, so the name it will take must not be given out twice meanwhile.
    static DOWNLOADING: RefCell<HashMap<u32, PathBuf>> = RefCell::default();
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

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(PaneDownload::new())
        }
    }
}

wrap_download_handler! {
    struct PaneDownload;

    // Without a handler Chromium cancels every download.
    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let (Some(item), Some(name), Some(callback)) = (download_item, suggested_name, callback)
            else {
                return 0;
            };
            let folder = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads"));
            let taken = |path: &Path| {
                path.exists()
                    || DOWNLOADING.with_borrow(|downloading| downloading.values().any(|p| p == path))
            };
            let Some(path) = folder.and_then(|folder| free_name(&folder, &name.to_string(), taken))
            else {
                return 0;
            };
            callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
            DOWNLOADING.with_borrow_mut(|downloading| downloading.insert(item.id(), path));
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            _callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else {
                return;
            };
            let done = item.is_complete() != 0;
            if done || item.is_canceled() != 0 || item.is_interrupted() != 0 {
                DOWNLOADING.with_borrow_mut(|downloading| downloading.remove(&item.id()));
            }
            if done {
                mac::download_finished(&CefString::from(&item.full_path()).to_string());
            }
        }
    }
}

/// Where a download named `name` goes in `folder`: under that name, or numbered as Chrome
/// does ("a (1).pdf") when it is `taken`. Folders in the name are dropped.
fn free_name(folder: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let name = Path::new(name).file_name()?.to_str()?;
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    std::iter::once(folder.join(name))
        .chain((1..1000).map(|n| folder.join(format!("{stem} ({n}){extension}"))))
        .find(|path| !taken(path))
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
            let quiet = AGENT_MENUS.try_with(|menus| {
                let at = menus.try_borrow_mut().ok()?.remove(&id)?;
                Some(at.elapsed() < Duration::from_secs(3))
            });
            // An empty menu is not shown.
            if quiet == Ok(Some(true)) {
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

#[cfg(test)]
mod tests {
    use super::free_name;

    #[test]
    fn downloads_never_overwrite_and_stay_in_the_folder() {
        let folder =
            std::env::temp_dir().join(format!("bareframe-downloads-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let exists = |path: &std::path::Path| path.exists();
        assert_eq!(
            free_name(&folder, "a.pdf", exists),
            Some(folder.join("a.pdf"))
        );
        std::fs::write(folder.join("a.pdf"), "").unwrap();
        std::fs::write(folder.join("a (1).pdf"), "").unwrap();
        assert_eq!(
            free_name(&folder, "a.pdf", exists),
            Some(folder.join("a (2).pdf"))
        );
        // A name another download is still writing to is taken too.
        let writing = folder.join("a (2).pdf");
        let busy = |path: &std::path::Path| path.exists() || path == writing;
        assert_eq!(
            free_name(&folder, "a.pdf", busy),
            Some(folder.join("a (3).pdf"))
        );
        assert_eq!(
            free_name(&folder, "../../etc/passwd", exists),
            Some(folder.join("passwd"))
        );
        assert_eq!(free_name(&folder, "..", exists), None);
        std::fs::write(folder.join("README"), "").unwrap();
        assert_eq!(
            free_name(&folder, "README", exists),
            Some(folder.join("README (1)"))
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
