//! Programs in bareframe's terminals (Claude Code, Codex) reach the browser tabs through
//! here: `bareframe-browser` connects to a Unix socket (see protocol.rs), and its requests
//! are answered on the UI thread, where the tabs and Chromium live. Off unless Settings ›
//! Features › Browser turns it on; terminals opened meanwhile learn the socket's path and
//! their own id.
//!
//! The user allows a terminal's agent to use a tab in the agent itself: Claude Code asks
//! before the first use, through `bareframe-browser hook` (which reads `granted` here). The
//! user manages the agent's tabs by telling it: it releases tabs, and opens and closes its own.
use super::{BrowserPane, automation, automation::Failure, engine, pane::address_for, protocol};
use crate::workspace::WorkspaceView;
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{AnyWindowHandle, App, AsyncApp, Entity, WeakEntity};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io::{BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

type Request = (Value, oneshot::Sender<Value>);

/// A browser pane and the id agents know it by.
#[derive(Clone)]
struct Tab {
    id: String,
    pane: WeakEntity<BrowserPane>,
}

#[derive(Default)]
struct State {
    next_tab: u64,
    tabs: Vec<Tab>,
    next_terminal: u64,
    /// Windows a tab can be opened in.
    workspaces: Vec<(WeakEntity<WorkspaceView>, AnyWindowHandle)>,
    /// (terminal, tab) pairs in use: the tab was used from that terminal, or opened there.
    grants: HashSet<(String, String)>,
    /// Tabs a terminal opened, which it may also close.
    owners: HashMap<String, String>,
    /// Documents each browser has loaded, counted from when it was first asked for: a
    /// snapshot's references hold only for the document it was taken in.
    documents: HashMap<i32, u64>,
}

struct Endpoint {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    _serve: gpui::Task<()>,
    _events: gpui::Task<()>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
    static ENDPOINT: RefCell<Option<Endpoint>> = const { RefCell::new(None) };
}

/// Gives a new browser pane the id agents use for it, for the app's run.
pub fn register(pane: WeakEntity<BrowserPane>) -> String {
    STATE.with_borrow_mut(|state| {
        state.next_tab += 1;
        let id = format!("tab-{}", state.next_tab);
        state.tabs.retain(|tab| tab.pane.upgrade().is_some());
        state.tabs.push(Tab {
            id: id.clone(),
            pane,
        });
        id
    })
}

/// A window agents may open tabs in.
pub fn register_workspace(view: WeakEntity<WorkspaceView>, window: AnyWindowHandle) {
    STATE.with_borrow_mut(|state| {
        state
            .workspaces
            .retain(|(view, _)| view.upgrade().is_some());
        state.workspaces.push((view, window));
    });
}

fn find(state: &State, tab: &str) -> Option<Tab> {
    state.tabs.iter().find(|known| known.id == tab).cloned()
}

/// `/usr/bin/env` with the socket and a new terminal's id, while control is on.
pub fn launch_env() -> Option<String> {
    let path = ENDPOINT.with_borrow(|endpoint| endpoint.as_ref().map(|e| e.path.clone()))?;
    let terminal = STATE.with_borrow_mut(|state| {
        state.next_terminal += 1;
        format!("terminal-{}", state.next_terminal)
    });
    let quote = bareframe_core::config::quote;
    let mut env = format!(
        "/usr/bin/env {}={} {}={}",
        protocol::ENDPOINT_VAR,
        quote(path.to_str()?),
        protocol::TERMINAL_VAR,
        quote(&terminal)
    );
    let cli = std::env::current_exe()
        .ok()?
        .with_file_name("bareframe-browser");
    if let Some(cli) = cli.to_str().filter(|_| cli.exists()) {
        env.push_str(&format!(" {}={}", protocol::CLI_VAR, quote(cli)));
    }
    Some(env)
}

/// Opens or closes the socket, following the setting.
pub fn set_enabled(enabled: bool, cx: &mut App) {
    if enabled == ENDPOINT.with_borrow(Option::is_some) {
        return;
    }
    if !enabled {
        if let Some(endpoint) = ENDPOINT.take() {
            endpoint.stop.store(true, Ordering::Relaxed);
            // Wakes the listener so it sees the flag and ends.
            let _ = UnixStream::connect(&endpoint.path);
            let _ = std::fs::remove_file(&endpoint.path);
        }
        return;
    }
    // The temporary folder macOS gives each user is readable by that user alone.
    let path = std::env::temp_dir().join(format!("bareframe-browser-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("bareframe: browser control {}: {error}", path.display());
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let (requests, mut incoming) = mpsc::unbounded::<Request>();
    let stop = Arc::new(AtomicBool::new(false));
    std::thread::spawn({
        let stop = stop.clone();
        move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(stream) = stream {
                    let requests = requests.clone();
                    std::thread::spawn(move || serve(stream, requests));
                }
            }
        }
    });
    let serve = cx.spawn(async move |cx| {
        while let Some((request, reply)) = incoming.next().await {
            cx.spawn(async move |cx| {
                let _ = reply.send(answer(request, cx).await);
            })
            .detach();
        }
    });
    let (sink, mut events) = mpsc::unbounded();
    engine::set_events(sink);
    let events = cx.spawn(async move |_| {
        while let Some(event) = events.next().await {
            on_event(event);
        }
    });
    cx.on_app_quit({
        let path = path.clone();
        move |_| {
            let _ = std::fs::remove_file(&path);
            async {}
        }
    })
    .detach();
    ENDPOINT.set(Some(Endpoint {
        path,
        stop,
        _serve: serve,
        _events: events,
    }));
}

/// One connection: a line in, a line out, until the other side hangs up.
fn serve(stream: UnixStream, requests: mpsc::UnboundedSender<Request>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    while let Ok(Some(line)) = protocol::read_line(&mut reader) {
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => {
                let (sender, reply) = oneshot::channel();
                if requests.unbounded_send((request, sender)).is_err() {
                    return;
                }
                match futures::executor::block_on(reply) {
                    Ok(reply) => reply,
                    Err(_) => return,
                }
            }
            Err(error) => failure(Value::Null, ("BAD_REQUEST", error.to_string())),
        };
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

async fn answer(request: Value, cx: &mut AsyncApp) -> Value {
    let id = request["id"].clone();
    let params = &request["params"];
    let result = match request["method"].as_str().unwrap_or_default() {
        "tabs" => Ok(tabs(cx)),
        "granted" => granted(params, cx),
        "open" => open(params, cx).await,
        method => on_tab(method, params, cx).await,
    };
    match result {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(error) => failure(id, error),
    }
}

fn failure(id: Value, (code, message): Failure) -> Value {
    json!({ "id": id, "error": { "code": code, "message": message } })
}

fn tabs(cx: &mut AsyncApp) -> Value {
    let tabs = STATE.with_borrow(|state| state.tabs.clone());
    let list: Vec<Value> = tabs
        .into_iter()
        .filter_map(|tab| {
            tab.pane
                .read_with(cx, |pane, _| {
                    json!({
                        "tab": tab.id,
                        "title": pane.title(),
                        "url": pane.url(),
                        "ready": pane.browser_id().is_some(),
                        "visible": pane.is_visible(),
                        "loading": pane.is_loading(),
                    })
                })
                .ok()
        })
        .collect();
    Value::Array(list)
}

/// Whether this terminal uses `tab` already, for the hook deciding to ask the user; and
/// what the tab shows, for the question.
fn granted(params: &Value, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let pair = (
        terminal_of(params),
        params["tab"].as_str().unwrap_or_default().to_owned(),
    );
    let (granted, tab) =
        STATE.with_borrow(|state| (state.grants.contains(&pair), find(state, &pair.1)));
    let tab = tab.ok_or(("TAB_NOT_FOUND", format!("no tab `{}`", pair.1)))?;
    let (title, url) = tab
        .pane
        .read_with(cx, |pane, _| (pane.title(), pane.url().to_owned()))
        .map_err(|_| ("TAB_CLOSED", format!("`{}` is closed", pair.1)))?;
    Ok(json!({ "tab": pair.1, "granted": granted, "title": title, "url": url }))
}

fn terminal_of(params: &Value) -> String {
    params["terminal"].as_str().unwrap_or_default().to_owned()
}

/// A request about one tab: allowed first, then done.
async fn on_tab(method: &str, params: &Value, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let known = [
        "snapshot",
        "screenshot",
        "navigate",
        "click",
        "fill",
        "press_key",
        "wait_for",
        "release",
        "close",
    ];
    if !known.contains(&method) {
        return Err(("UNKNOWN_METHOD", format!("no method `{method}`")));
    }
    let terminal = terminal_of(params);
    let tab = params["tab"]
        .as_str()
        .ok_or(("BAD_REQUEST", "no `tab`; list them with `tabs`".to_owned()))?;
    if method == "release" {
        STATE.with_borrow_mut(|state| state.grants.remove(&(terminal, tab.to_owned())));
        return Ok(json!({ "tab": tab, "released": true }));
    }
    let known = STATE
        .with_borrow(|state| find(state, tab))
        .ok_or(("TAB_NOT_FOUND", format!("no tab `{tab}`")))?;
    // Claude Code asked the user already, before the first use (see the module's note).
    STATE.with_borrow_mut(|state| state.grants.insert((terminal.clone(), known.id.clone())));
    if method == "close" {
        return close(&terminal, &known, cx);
    }
    let browser = known
        .pane
        .read_with(cx, |pane, _| pane.browser_id())
        .map_err(|_| ("TAB_CLOSED", format!("`{tab}` is closed")))?
        .ok_or((
            "TAB_NOT_READY",
            format!("`{tab}` has no page yet: show it once"),
        ))?;
    watch(browser, cx).await?;
    // Actions on a snapshot's refs check it is still the same document.
    if let Some(expected) = params["document"].as_u64()
        && expected != document(browser)
    {
        return Err((
            "STALE_DOCUMENT",
            format!("the page changed since document {expected}; take a new snapshot"),
        ));
    }
    let node = || {
        params["ref"].as_i64().ok_or((
            "BAD_REQUEST",
            "no `ref`; take one from a snapshot".to_owned(),
        ))
    };
    let mut result = json!({ "tab": tab });
    match method {
        "snapshot" => {
            let (text, truncated) = automation::snapshot(browser, cx).await?;
            result["text"] = Value::from(text);
            result["truncated"] = Value::from(truncated);
        }
        "screenshot" => {
            result["format"] = Value::from("png");
            result["data"] = automation::screenshot(browser, cx).await?;
        }
        "navigate" => {
            let url = params["url"]
                .as_str()
                .ok_or(("BAD_REQUEST", "no `url`".to_owned()))?;
            automation::navigate(browser, &address_for(url), cx).await?;
        }
        "click" => automation::click(browser, node()?, cx).await?,
        "fill" => {
            let text = params["text"]
                .as_str()
                .ok_or(("BAD_REQUEST", "no `text`".to_owned()))?;
            automation::fill(browser, node()?, text, cx).await?;
        }
        "press_key" => {
            let key = params["key"]
                .as_str()
                .ok_or(("BAD_REQUEST", "no `key`".to_owned()))?;
            automation::press(browser, key, cx).await?;
        }
        "wait_for" => {
            let text = params["text"]
                .as_str()
                .ok_or(("BAD_REQUEST", "no `text`".to_owned()))?;
            let limit = params["timeout_ms"]
                .as_u64()
                .map_or(automation::WAIT, Duration::from_millis);
            automation::wait_for_text(browser, text, limit, cx).await?;
            result["found"] = Value::from(true);
        }
        _ => unreachable!("checked above"),
    }
    // What the page is now: actions can move it to another document.
    result["document"] = Value::from(document(browser));
    Ok(result)
}

/// Opens `url` in a new browser tab, owned by the asking terminal, and waits for it to load.
async fn open(params: &Value, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let url = address_for(
        params["url"]
            .as_str()
            .ok_or(("BAD_REQUEST", "no `url`".to_owned()))?,
    );
    let terminal = terminal_of(params);
    let (view, window) = STATE
        .with_borrow(|state| {
            state
                .workspaces
                .iter()
                .find(|(view, _)| view.upgrade().is_some())
                .cloned()
        })
        .ok_or(("NO_WINDOW", "bareframe has no window open".to_owned()))?;
    let (sender, opened) = oneshot::channel::<Entity<BrowserPane>>();
    let open = {
        let mut cx = cx.clone();
        move || {
            let pane = window.update(&mut cx, |_, window, cx| {
                view.update(cx, |view, cx| view.open_agent_tab(url, window, cx))
            });
            if let Ok(Ok(pane)) = pane {
                let _ = sender.send(pane);
            }
        }
    };
    // Chromium starts only between events (see engine::start), as from a menu.
    if engine::needs_start() && super::in_run_loop() {
        super::between_events(open).map_err(|error| ("OPEN_FAILED", error))?;
    } else {
        open();
    }
    let pane = opened
        .await
        .map_err(|_| ("OPEN_FAILED", "the window went away".to_owned()))?;
    let tab = pane.read_with(cx, |pane, _| pane.tab().to_owned());
    STATE.with_borrow_mut(|state| {
        state.grants.insert((terminal.clone(), tab.clone()));
        state.owners.insert(tab.clone(), terminal);
    });
    // The page exists once its tab has been laid out, a frame or so away.
    let start = std::time::Instant::now();
    let browser = loop {
        if let Some(browser) = pane.read_with(cx, |pane, _| pane.browser_id()) {
            break browser;
        }
        if let Some(error) = pane.read_with(cx, |pane, _| pane.error()) {
            return Err(("OPEN_FAILED", error));
        }
        if start.elapsed() > automation::WAIT {
            return Err(("TIMEOUT", format!("`{tab}` did not get a page")));
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    };
    watch(browser, cx).await?;
    automation::loaded(browser, cx).await?;
    Ok(json!({ "tab": tab, "document": document(browser) }))
}

/// Closes a tab the terminal opened; a tab the user opened stays theirs to close.
fn close(terminal: &str, tab: &Tab, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let owner = STATE.with_borrow(|state| state.owners.get(&tab.id).cloned());
    if owner.as_deref() != Some(terminal) {
        return Err((
            "PERMISSION_DENIED",
            "only tabs this terminal opened can be closed; release this one instead".into(),
        ));
    }
    let Some(pane) = tab.pane.upgrade() else {
        return Err(("TAB_CLOSED", format!("`{}` is closed", tab.id)));
    };
    let views = STATE.with_borrow(|state| state.workspaces.clone());
    for (view, window) in views {
        let _ = window.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.close_browser_pane(&pane, window, cx))
        });
    }
    STATE.with_borrow_mut(|state| {
        state.owners.remove(&tab.id);
        state.grants.retain(|(_, granted)| *granted != tab.id);
    });
    Ok(json!({ "tab": tab.id, "closed": true }))
}

/// Starts counting `browser`'s documents and has its page act focused, once: input from an
/// agent reaches it while the user types in the terminal.
async fn watch(browser: i32, cx: &mut AsyncApp) -> Result<(), Failure> {
    if STATE.with_borrow(|state| state.documents.contains_key(&browser)) {
        return Ok(());
    }
    automation::call(browser, "Page.enable", json!({}), cx).await?;
    automation::call(
        browser,
        "Emulation.setFocusEmulationEnabled",
        json!({ "enabled": true }),
        cx,
    )
    .await?;
    STATE.with_borrow_mut(|state| {
        state.documents.entry(browser).or_insert(1);
    });
    Ok(())
}

fn on_event(event: engine::Event) {
    if event.method != "Page.frameNavigated" {
        return;
    }
    let Ok(params) = serde_json::from_slice::<Value>(&event.params) else {
        return;
    };
    // The page itself, not a frame inside it.
    if params["frame"]["parentId"].is_null() {
        STATE.with_borrow_mut(|state| {
            if let Some(count) = state.documents.get_mut(&event.browser) {
                *count += 1;
            }
        });
    }
}

fn document(browser: i32) -> u64 {
    STATE.with_borrow(|state| state.documents.get(&browser).copied().unwrap_or(0))
}
