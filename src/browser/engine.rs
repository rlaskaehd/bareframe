//! Chromium itself (CEF): loaded and started when the first browser pane opens, run from
//! GPUI's loop when Chromium asks, and shut down when the app quits. It owns every browser,
//! so none outlives the shutdown; panes hold only an id. A build run without the framework
//! beside it (a plain `cargo run`, before script/bundle) has no browser, and panes say so.
//!
//! Panes are driven over the DevTools protocol through `devtools`, inside the process: no
//! port is opened.
use super::{client, mac};
use cef::*;
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
    future::Either,
};
use gpui::App as Gpui;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

/// The longest Chromium's work waits when it hasn't asked for a turn.
const IDLE: Duration = Duration::from_millis(33);
/// How long quitting waits for the browsers to close before shutting Chromium down anyway.
const CLOSING: Duration = Duration::from_secs(3);

type Reply = oneshot::Sender<Result<serde_json::Value, String>>;

#[derive(Default)]
struct Engine {
    /// Open browsers by id; one leaves as it starts closing.
    browsers: HashMap<i32, Browser>,
    /// Browsers made and not yet gone (`on_before_close`), closing ones included.
    live: usize,
    /// Each browser's DevTools observer, kept registered while it is open.
    observers: HashMap<i32, Registration>,
    /// Where pages keep cookies, history and caches: memory only, gone when the app quits.
    /// Chromium would encrypt saved cookies with the fixed key (see BrowserApp), which any
    /// program of the user's could read.
    /// ponytail: a profile on disk with a Keychain-held key when logins must survive quitting.
    memory: Option<RequestContext>,
    /// DevTools calls waiting for their result, by message id, with the browser asked.
    replies: HashMap<i32, (i32, Reply)>,
    next_message: i32,
}

thread_local! {
    /// How starting went, once tried; Chromium starts at most once per process.
    static STARTED: RefCell<Option<Result<(), String>>> = const { RefCell::new(None) };
    /// Every borrow is short and never spans a call into Chromium, which may call back.
    static ENGINE: RefCell<Engine> = RefCell::default();
    /// Chromium's work is running; a nested loop inside it doesn't run it again.
    static PUMPING: Cell<bool> = const { Cell::new(false) };
    /// Chromium is shut down: nothing may call it again.
    static DOWN: Cell<bool> = const { Cell::new(false) };
    /// Where DevTools events go (see `set_events`), if anywhere.
    static EVENTS: RefCell<Option<mpsc::UnboundedSender<Event>>> = const { RefCell::new(None) };
}

/// Chromium's requests for a turn, from any of its threads: in how many milliseconds.
static PUMP: OnceLock<mpsc::UnboundedSender<i64>> = OnceLock::new();

/// Has macOS check Chromium's framework in a helper process off the main thread, so the
/// first browser pane loads it at once (see bareframe-helper). Builds without it do nothing.
pub fn warm_up() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let helper =
        exe.with_file_name("../Frameworks/bareframe Helper.app/Contents/MacOS/bareframe Helper");
    if helper.exists() {
        std::thread::spawn(move || {
            let _ = std::process::Command::new(helper)
                .arg("--bareframe-warm-up")
                .status();
        });
    }
}

/// Whether Chromium is still to start, which must happen outside a run loop pass.
pub fn needs_start() -> bool {
    STARTED.with_borrow(Option::is_none)
}

/// Starts Chromium if it hasn't been, keeping its own files (not the pages') in `data`.
pub fn start(data: &Path, cx: &mut Gpui) -> Result<(), String> {
    if let Some(started) = STARTED.with_borrow(Clone::clone) {
        return started;
    }
    // Chromium watches the run loop's passes begin and end; started during one, it sees an
    // end it never saw begin and crashes. It may be asked again from a better place.
    if mac::in_run_loop() {
        return Err(
            "Chromium can only start from the app's launch or your action. Open the tab again."
                .into(),
        );
    }
    let started = start_now(data, cx);
    if let Err(error) = &started {
        eprintln!("bareframe: browser: {error}");
    }
    STARTED.set(Some(started.clone()));
    started
}

fn start_now(data: &Path, cx: &mut Gpui) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let loader = library_loader::LibraryLoader::new(&executable, false);
    if !loader.load() {
        return Err("This build has no Chromium. Make the app with script/bundle.".into());
    }
    // Chromium stays loaded for the life of the process.
    std::mem::forget(loader);
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    mac::teach_application()?;
    if let Err(error) = std::fs::create_dir_all(data).and_then(|()| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700))
    }) {
        eprintln!("bareframe: {}: {error}", data.display());
    }
    let data = CefString::from(data.to_string_lossy().as_ref());
    let settings = Settings {
        // Pages run in Chromium's sandbox (see bareframe-helper).
        no_sandbox: 0,
        // GPUI owns the run loop; Chromium's work is run from it below.
        external_message_pump: 1,
        root_cache_path: data.clone(),
        cache_path: data,
        ..Default::default()
    };
    let (requests, mut turns) = mpsc::unbounded();
    let _ = PUMP.set(requests);
    let arguments = args::Args::new();
    if initialize(
        Some(arguments.as_main_args()),
        Some(&settings),
        Some(&mut BrowserApp::new()),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("Chromium could not start".into());
    }
    cx.spawn(async move |cx| {
        let mut due = Instant::now();
        loop {
            let wait = cx
                .background_executor()
                .timer(due.saturating_duration_since(Instant::now()));
            match futures::future::select(turns.next(), wait).await {
                Either::Left((Some(delay), _)) => {
                    let delay = Duration::from_millis(delay.max(0).unsigned_abs());
                    due = due.min(Instant::now() + delay);
                    continue;
                }
                Either::Left((None, _)) => return,
                Either::Right(_) => {}
            }
            if DOWN.get() {
                return;
            }
            // Not through the app: Chromium may run a nested loop that lands here while the
            // app is busy (quitting, say).
            if !PUMPING.replace(true) {
                do_message_loop_work();
                PUMPING.set(false);
            }
            due = Instant::now() + IDLE;
        }
    })
    .detach();
    cx.on_app_quit(|_| {
        shut_down();
        async {}
    })
    .detach();
    Ok(())
}

wrap_app! {
    struct BrowserApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Chromium keeps its cookie key in the login Keychain, which asks the user each
            // time an unsigned build changes; a fixed key instead, and no prompt. Without it,
            // the prompt's nested run loop also re-enters Chromium and crashes it. The helper
            // processes add it too.
            let Some(command_line) = command_line else {
                return;
            };
            command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
            // Memory: no renderer started ahead for the next tab,
            // no pages kept alive for Back, one renderer per site across tabs, and none of
            // Chrome's background downloads (components, field trials) a pane has no use for.
            for switch in [
                "process-per-site",
                "disable-component-update",
                "disable-background-networking",
            ] {
                command_line.append_switch(Some(&CefString::from(switch)));
            }
            command_line.append_switch_with_value(
                Some(&CefString::from("disable-features")),
                Some(&CefString::from("SpareRendererForSitePerProcess,BackForwardCache")),
            );
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(Pump::new())
        }
    }
}

wrap_browser_process_handler! {
    struct Pump;

    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            if let Some(pump) = PUMP.get() {
                let _ = pump.unbounded_send(delay_ms);
            }
        }

        fn on_already_running_app_relaunch(
            &self,
            _command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            // A second bareframe using the same browser folder (a dev build beside the
            // installed one) can't start Chromium; left to itself, Chromium here would answer
            // it with a Chrome window of its own. That bareframe's panes say what failed.
            1
        }
    }
}

wrap_dev_tools_message_observer! {
    struct Results;

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: ::std::os::raw::c_int,
            success: ::std::os::raw::c_int,
            result: Option<&[u8]>,
        ) {
            // `try_` all the way: a panic in Chromium's callback would abort the app.
            let reply = ENGINE.try_with(|engine| {
                engine.try_borrow_mut().ok()?.replies.remove(&message_id)
            });
            let Ok(Some((_, reply))) = reply else {
                return;
            };
            let result = result.unwrap_or(b"{}");
            let _ = reply.send(if success != 0 {
                serde_json::from_slice(result).map_err(|error| error.to_string())
            } else {
                Err(String::from_utf8_lossy(result).into_owned())
            });
        }

        fn on_dev_tools_event(
            &self,
            browser: Option<&mut Browser>,
            method: Option<&CefString>,
            params: Option<&[u8]>,
        ) {
            let (Some(browser), Some(method)) = (browser, method) else {
                return;
            };
            emit(Event {
                browser: browser.identifier(),
                method: method.to_string(),
                params: params.unwrap_or(b"{}").to_vec(),
            });
        }
    }
}

/// A DevTools event a page sent (`Page.frameNavigated` and so on): only the domains asked
/// for with their `enable` method send any.
pub struct Event {
    pub browser: i32,
    pub method: String,
    /// The event's JSON parameters, parsed by whoever wants them, off Chromium's callback.
    pub params: Vec<u8>,
}

/// The method of the event a browser sends last, as it closes: what was kept about it can go.
pub const CLOSED: &str = "bareframe.closed";

/// Sends every DevTools event from now on to `events`.
pub fn set_events(events: mpsc::UnboundedSender<Event>) {
    EVENTS.set(Some(events));
}

fn emit(event: Event) {
    let _ = EVENTS.try_with(|events| {
        if let Ok(events) = events.try_borrow()
            && let Some(events) = events.as_ref()
        {
            let _ = events.unbounded_send(event);
        }
    });
}

/// Opens a browser as a child of `parent` (GPUI's view), its events going to `events`.
pub fn open(
    parent: *mut std::ffi::c_void,
    url: &str,
    events: mpsc::UnboundedSender<client::BrowserEvent>,
    keyboard: client::Keyboard,
) -> Option<i32> {
    if DOWN.get() {
        return None;
    }
    let info = WindowInfo {
        runtime_style: RuntimeStyle::ALLOY,
        ..Default::default()
    }
    .set_as_child(parent.cast(), &Rect::default());
    let mut memory = ENGINE
        .with_borrow(|engine| engine.memory.clone())
        .or_else(|| {
            // An empty cache path keeps the context in memory.
            request_context_create_context(Some(&RequestContextSettings::default()), None)
        })?;
    ENGINE.with_borrow_mut(|engine| engine.memory = Some(memory.clone()));
    let browser = browser_host_create_browser_sync(
        Some(&info),
        Some(&mut client::client(events, keyboard)),
        Some(&CefString::from(url)),
        Some(&BrowserSettings::default()),
        None,
        Some(&mut memory),
    )?;
    let id = browser.identifier();
    let observer = browser
        .host()
        .and_then(|host| host.add_dev_tools_message_observer(Some(&mut Results::new())));
    ENGINE.with_borrow_mut(|engine| {
        engine.live += 1;
        engine.browsers.insert(id, browser);
        engine
            .observers
            .extend(observer.map(|observer| (id, observer)));
    });
    Some(id)
}

/// The open browser `id`, to call into outside the engine's borrow.
pub fn browser(id: i32) -> Option<Browser> {
    if DOWN.get() {
        return None;
    }
    ENGINE.with_borrow(|engine| engine.browsers.get(&id).cloned())
}

/// The host of browser `id`, while it is open, for focus and closing.
pub fn host(id: i32) -> Option<BrowserHost> {
    browser(id)?.host()
}

/// The page's view of browser `id`, while it is open.
pub fn view(id: i32) -> Option<*mut std::ffi::c_void> {
    let view = host(id)?.window_handle();
    (!view.is_null()).then_some(view)
}

/// Browser `id` started closing (by itself, or from `close`): panes stop using it.
pub(super) fn closing(id: i32) {
    let gone = ENGINE.with_borrow_mut(|engine| {
        engine.observers.remove(&id);
        // Calls waiting on it end now ("the page closed"), not never.
        engine.replies.retain(|_, (browser, _)| *browser != id);
        engine.browsers.remove(&id)
    });
    drop(gone);
    emit(Event {
        browser: id,
        method: CLOSED.into(),
        params: Vec::new(),
    });
}

/// Browser `id` is gone.
pub(super) fn closed() {
    ENGINE.with_borrow_mut(|engine| engine.live = engine.live.saturating_sub(1));
}

/// Closes browser `id` for good.
pub fn close(id: i32) {
    if let Some(browser) = browser(id) {
        closing(id);
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }
}

/// Runs a DevTools `method` on browser `id`, inside the process, and resolves to its result.
/// Dropping the future (a timeout, say) forgets the call; a late result is ignored.
pub fn devtools(
    id: i32,
    method: &str,
    params: serde_json::Value,
) -> impl Future<Output = Result<serde_json::Value, String>> + use<> {
    let (reply, result) = oneshot::channel();
    let sent = browser(id).and_then(|browser| {
        let host = browser.host()?;
        let message_id = ENGINE.with_borrow_mut(|engine| {
            engine.next_message += 1;
            engine.replies.insert(engine.next_message, (id, reply));
            engine.next_message
        });
        let message = serde_json::json!({ "id": message_id, "method": method, "params": params });
        if host.send_dev_tools_message(Some(message.to_string().as_bytes())) != 0 {
            Some(Pending(message_id))
        } else {
            ENGINE.with_borrow_mut(|engine| engine.replies.remove(&message_id));
            None
        }
    });
    async move {
        let _pending = sent.ok_or("the page is closed")?;
        result.await.map_err(|_| "the page closed".to_owned())?
    }
}

/// A DevTools call in flight; dropped unanswered, it leaves the engine's list.
struct Pending(i32);

impl Drop for Pending {
    fn drop(&mut self) {
        let _ = ENGINE.try_with(|engine| {
            if let Ok(mut engine) = engine.try_borrow_mut() {
                engine.replies.remove(&self.0);
            }
        });
    }
}

/// Closes every browser, lets Chromium finish closing them, then shuts it down.
fn shut_down() {
    PUMPING.set(true);
    let browsers: Vec<i32> = ENGINE.with_borrow(|engine| engine.browsers.keys().copied().collect());
    for id in browsers {
        close(id);
    }
    let until = Instant::now() + CLOSING;
    while ENGINE.with_borrow(|engine| engine.live) > 0 && Instant::now() < until {
        do_message_loop_work();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Chromium's objects are let go of before it shuts down, never after.
    drop(ENGINE.take());
    DOWN.set(true);
    shutdown();
}
