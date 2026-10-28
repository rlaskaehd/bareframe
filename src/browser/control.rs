//! Programs in bareframe's terminals (Claude Code, Codex) reach the browser tabs through
//! here: `bareframe-browser` connects to a Unix socket (see protocol.rs), and its requests
//! are answered on the UI thread, where the tabs and Chromium live. Off unless Settings ›
//! Features › Browser turns it on; terminals opened meanwhile learn the socket's path.
use super::{BrowserPane, engine, protocol};
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
    future::Either,
};
use gpui::{App, AsyncApp, WeakEntity};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::HashMap,
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

/// How long a page has to answer one DevTools call.
const DEADLINE: Duration = Duration::from_secs(10);
/// The most lines a snapshot gives; a page's whole tree can run to tens of thousands.
const SNAPSHOT_LINES: usize = 4000;

type Request = (Value, oneshot::Sender<Value>);
/// A browser pane and the id agents know it by.
type Tab = (String, WeakEntity<BrowserPane>);
type Failure = (&'static str, String);

struct Endpoint {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    _serve: gpui::Task<()>,
    _events: gpui::Task<()>,
}

thread_local! {
    /// Every browser pane made, by the id agents see; dead ones are dropped as new ones come.
    static TABS: RefCell<(u64, Vec<Tab>)> = const { RefCell::new((0, Vec::new())) };
    static ENDPOINT: RefCell<Option<Endpoint>> = const { RefCell::new(None) };
    /// Documents each browser has loaded, counted from when it was first asked for: a
    /// snapshot's references hold only for the document it was taken in.
    static DOCUMENTS: RefCell<HashMap<i32, u64>> = RefCell::new(HashMap::new());
}

/// Gives a new browser pane the id agents use for it, for the app's run.
pub fn register(pane: WeakEntity<BrowserPane>) {
    TABS.with_borrow_mut(|(next, tabs)| {
        *next += 1;
        tabs.retain(|(_, pane)| pane.upgrade().is_some());
        tabs.push((format!("tab-{next}"), pane));
    });
}

/// `/usr/bin/env` with the socket for a new terminal's command, while control is on.
pub fn launch_env() -> Option<String> {
    let path = ENDPOINT.with_borrow(|endpoint| endpoint.as_ref().map(|e| e.path.clone()))?;
    let quote = bareframe_core::config::quote;
    let mut env = format!(
        "/usr/bin/env {}={}",
        protocol::ENDPOINT_VAR,
        quote(path.to_str()?)
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
        "snapshot" => snapshot(params, cx).await,
        "screenshot" => screenshot(params, cx).await,
        method => Err(("UNKNOWN_METHOD", format!("no method `{method}`"))),
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
    let tabs = TABS.with_borrow(|(_, tabs)| tabs.clone());
    let list: Vec<Value> = tabs
        .into_iter()
        .filter_map(|(id, pane)| {
            pane.read_with(cx, |pane, _| {
                json!({
                    "tab": id,
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

/// The browser of the tab `params` names.
fn browser_of(params: &Value, cx: &mut AsyncApp) -> Result<i32, Failure> {
    let tab = params["tab"]
        .as_str()
        .ok_or(("BAD_REQUEST", "no `tab`; list them with `tabs`".to_owned()))?;
    let pane = TABS
        .with_borrow(|(_, tabs)| {
            tabs.iter()
                .find(|(id, _)| id == tab)
                .map(|(_, p)| p.clone())
        })
        .ok_or(("TAB_NOT_FOUND", format!("no tab `{tab}`")))?;
    pane.read_with(cx, |pane, _| pane.browser_id())
        .map_err(|_| ("TAB_CLOSED", format!("`{tab}` is closed")))?
        .ok_or((
            "TAB_NOT_READY",
            format!("`{tab}` has no page yet: show it once"),
        ))
}

/// A DevTools call on `browser`, within `DEADLINE`.
async fn call(
    browser: i32,
    method: &str,
    params: Value,
    cx: &mut AsyncApp,
) -> Result<Value, Failure> {
    let reply = engine::devtools(browser, method, params);
    let deadline = cx.background_executor().timer(DEADLINE);
    match futures::future::select(std::pin::pin!(reply), deadline).await {
        Either::Left((Ok(result), _)) => Ok(result),
        Either::Left((Err(error), _)) if error.starts_with("the page") => {
            Err(("TAB_CLOSED", error))
        }
        Either::Left((Err(error), _)) => Err(("DEVTOOLS_ERROR", error)),
        Either::Right(_) => Err(("TIMEOUT", format!("{method} took over {DEADLINE:?}"))),
    }
}

/// Starts counting `browser`'s documents, once.
async fn watch(browser: i32, cx: &mut AsyncApp) -> Result<(), Failure> {
    if DOCUMENTS.with_borrow(|documents| documents.contains_key(&browser)) {
        return Ok(());
    }
    call(browser, "Page.enable", json!({}), cx).await?;
    DOCUMENTS.with_borrow_mut(|documents| {
        documents.entry(browser).or_insert(1);
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
        DOCUMENTS.with_borrow_mut(|documents| {
            if let Some(count) = documents.get_mut(&event.browser) {
                *count += 1;
            }
        });
    }
}

fn document(browser: i32) -> u64 {
    DOCUMENTS.with_borrow(|documents| documents.get(&browser).copied().unwrap_or(0))
}

async fn snapshot(params: &Value, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let browser = browser_of(params, cx)?;
    watch(browser, cx).await?;
    let tree = call(browser, "Accessibility.getFullAXTree", json!({}), cx).await?;
    let (text, truncated) = outline(&tree["nodes"]);
    Ok(json!({
        "tab": params["tab"],
        "document": document(browser),
        "text": text,
        "truncated": truncated,
    }))
}

async fn screenshot(params: &Value, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let browser = browser_of(params, cx)?;
    watch(browser, cx).await?;
    let shot = call(
        browser,
        "Page.captureScreenshot",
        json!({ "format": "png" }),
        cx,
    )
    .await?;
    Ok(json!({
        "tab": params["tab"],
        "document": document(browser),
        "format": "png",
        "data": shot["data"],
    }))
}

/// The accessibility tree as an indented outline, one element a line:
/// `button "Save" [ref=12]`, where `ref` is the element's DOM node for later actions.
/// Nodes Chromium marks ignored, and roles that only group (`generic`, `none`), show their
/// children in their place. Returns the text and whether it was cut at `SNAPSHOT_LINES`.
fn outline(nodes: &Value) -> (String, bool) {
    let nodes = nodes.as_array().map(Vec::as_slice).unwrap_or_default();
    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|node| Some((node["nodeId"].as_str()?, node)))
        .collect();
    let mut lines = Vec::new();
    let mut truncated = false;
    // Depth-first from the roots, kept on a stack in reading order.
    let mut stack: Vec<(&Value, usize)> = nodes
        .iter()
        .filter(|node| node["parentId"].is_null())
        .rev()
        .map(|node| (node, 0))
        .collect();
    while let Some((node, depth)) = stack.pop() {
        let role = node["role"]["value"].as_str().unwrap_or_default();
        let name = node["name"]["value"].as_str().unwrap_or_default().trim();
        let shown = !node["ignored"].as_bool().unwrap_or(false)
            && !matches!(
                role,
                "generic" | "none" | "InlineTextBox" | "LineBreak" | ""
            )
            && !(role == "StaticText" && name.is_empty());
        let children = if shown { depth + 1 } else { depth };
        if shown {
            if lines.len() == SNAPSHOT_LINES {
                truncated = true;
                break;
            }
            let mut line = format!("{}{role}", "  ".repeat(depth));
            if !name.is_empty() {
                let name: String = name.chars().take(200).collect();
                line.push_str(&format!(" {}", Value::from(name)));
            }
            if let Some(node) = node["backendDOMNodeId"].as_i64() {
                line.push_str(&format!(" [ref={node}]"));
            }
            lines.push(line);
        }
        let ids = node["childIds"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for id in ids.iter().rev() {
            if let Some(child) = id.as_str().and_then(|id| by_id.get(id)) {
                stack.push((child, children));
            }
        }
    }
    (lines.join("\n"), truncated)
}

#[cfg(test)]
mod tests {
    use super::outline;
    use serde_json::json;

    #[test]
    fn outline_keeps_meaningful_nodes_in_reading_order() {
        let nodes = json!([
            {"nodeId": "1", "role": {"value": "RootWebArea"}, "name": {"value": "Fixture"},
             "childIds": ["2", "5"], "backendDOMNodeId": 1},
            {"nodeId": "2", "parentId": "1", "role": {"value": "generic"}, "childIds": ["3", "4"]},
            {"nodeId": "3", "parentId": "2", "role": {"value": "button"},
             "name": {"value": "Save \"now\""}, "backendDOMNodeId": 12},
            {"nodeId": "4", "parentId": "2", "role": {"value": "StaticText"}, "name": {"value": ""}},
            {"nodeId": "5", "parentId": "1", "ignored": true, "role": {"value": "paragraph"},
             "childIds": ["6"]},
            {"nodeId": "6", "parentId": "5", "role": {"value": "StaticText"},
             "name": {"value": "hello"}, "backendDOMNodeId": 20},
        ]);
        let (text, truncated) = outline(&nodes);
        assert_eq!(
            text,
            "RootWebArea \"Fixture\" [ref=1]\n  button \"Save \\\"now\\\"\" [ref=12]\n  StaticText \"hello\" [ref=20]"
        );
        assert!(!truncated);
    }
}
