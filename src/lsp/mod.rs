//! The app's language servers, run as Warp runs them: one per server and project root,
//! started once the user enables that server for the project, shared by every editor on a
//! file inside it, and stopped when its last file closes. Enabled projects are remembered
//! in `language-servers` beside the settings file.
mod client;

use bareframe_core::lsp::{self, Server};
use client::Client;
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Subscription, Task};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

/// A server binary and the project root it serves.
type Key = (&'static str, PathBuf);

/// Columns as the server counts them: bytes if it agreed to UTF-8, else UTF-16 units.
#[derive(Clone, Copy, PartialEq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

/// An error or warning (Warp drops the rest), in the server's line and column units.
pub struct Diagnostic {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub error: bool,
    pub message: String,
}

pub enum State {
    Starting,
    Ready,
    Failed(String),
}

/// What the editor's language footer shows for a file.
pub enum Status<'a> {
    Checking,
    Install(&'static Server),
    /// The server is installed but not enabled for the project, named here.
    Enable(&'static Server, String),
    Running {
        server: &'static Server,
        state: &'a State,
        progress: Option<&'a str>,
    },
}

enum Installed {
    Checking,
    Found(PathBuf),
    Missing,
}

struct Running {
    /// `None` when the process couldn't start; the state says why.
    client: Option<Client>,
    state: State,
    encoding: Encoding,
    /// Work the server reports by progress token, oldest first.
    progress: Vec<(String, String)>,
    replies: HashMap<i64, oneshot::Sender<Result<Value, String>>>,
    _messages: Option<Task<()>>,
}

impl Running {
    fn ready(&self) -> Option<&Client> {
        matches!(self.state, State::Ready)
            .then_some(self.client.as_ref())
            .flatten()
    }
}

struct Document {
    key: Key,
    language: &'static str,
    version: i32,
    text: String,
    diagnostics: Vec<Diagnostic>,
}

pub struct LanguageServers {
    enabled: HashSet<Key>,
    /// Per project: rustup's proxies pick a toolchain by directory, and one project's
    /// toolchain may have rust-analyzer where another's lacks it. Until the user enables a
    /// project, the server is only tried from the home folder: run inside the project,
    /// rustup would obey the project's `rust-toolchain.toml`, which can name a program in
    /// the repository, and merely opening a file would run it.
    installed: HashMap<Key, Installed>,
    running: HashMap<Key, Running>,
    documents: HashMap<PathBuf, Document>,
    /// Looks again for a server being installed in a terminal.
    _recheck: Option<Task<()>>,
    _quit: Subscription,
}

pub enum LspEvent {
    /// A server's state, progress or a file's diagnostics changed.
    Updated,
}

impl EventEmitter<LspEvent> for LanguageServers {}

struct GlobalServers(Entity<LanguageServers>);

impl Global for GlobalServers {}

pub fn init(cx: &mut App) {
    let servers = cx.new(|cx| LanguageServers {
        enabled: load_enabled(),
        installed: HashMap::new(),
        running: HashMap::new(),
        documents: HashMap::new(),
        _recheck: None,
        // Dropping the clients asks every server to exit.
        _quit: cx.on_app_quit(|servers: &mut LanguageServers, _| {
            servers.running.clear();
            async {}
        }),
    });
    cx.set_global(GlobalServers(servers));
}

pub fn global(cx: &App) -> Entity<LanguageServers> {
    cx.global::<GlobalServers>().0.clone()
}

fn key_for(path: &Path) -> Option<(Key, &'static Server, &'static str)> {
    let (server, language) = lsp::server_for(path)?;
    Some(((server.binary, lsp::project_root(path)), server, language))
}

impl LanguageServers {
    /// An editor opened `path` holding `text`.
    pub fn open(&mut self, path: &Path, text: &str, cx: &mut Context<Self>) {
        let Some((key, server, language)) = key_for(path) else {
            return;
        };
        let document = Document {
            key: key.clone(),
            language,
            version: 1,
            text: text.to_owned(),
            diagnostics: Vec::new(),
        };
        if let Some(client) = self.running.get(&key).and_then(Running::ready) {
            did_open(client, path, &document);
        }
        self.documents.insert(path.to_path_buf(), document);
        self.check_installed(server, key.clone(), cx);
        if self.enabled.contains(&key) {
            self.start(server, key, cx);
        }
    }

    /// The editor's text changed; the whole text is sent, which every server accepts.
    pub fn change(&mut self, path: &Path, text: &str, cx: &mut Context<Self>) {
        let Some(document) = self.documents.get_mut(path) else {
            return;
        };
        document.version += 1;
        document.text = text.to_owned();
        // As in Warp, old marks would sit on the wrong text until the server answers.
        document.diagnostics.clear();
        if let Some(client) = self.running.get(&document.key).and_then(Running::ready) {
            client.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": lsp::path_to_uri(path), "version": document.version},
                    "contentChanges": [{"text": text}],
                }),
            );
        }
        cx.emit(LspEvent::Updated);
    }

    /// Saving lets servers such as rust-analyzer run their slower checks.
    pub fn saved(&self, path: &Path) {
        if let Some(client) = self
            .documents
            .get(path)
            .and_then(|document| self.running.get(&document.key))
            .and_then(Running::ready)
        {
            client.notify(
                "textDocument/didSave",
                json!({"textDocument": {"uri": lsp::path_to_uri(path)}}),
            );
        }
    }

    pub fn close(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(document) = self.documents.remove(path) else {
            return;
        };
        if let Some(client) = self.running.get(&document.key).and_then(Running::ready) {
            client.notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": lsp::path_to_uri(path)}}),
            );
        }
        if !self.documents.values().any(|open| open.key == document.key) {
            self.running.remove(&document.key);
        }
        cx.emit(LspEvent::Updated);
    }

    pub fn status(&self, path: &Path) -> Option<Status<'_>> {
        let (key, server, _) = key_for(path)?;
        if let Some(running) = self.running.get(&key) {
            return Some(Status::Running {
                server,
                state: &running.state,
                progress: running.progress.last().map(|(_, text)| text.as_str()),
            });
        }
        Some(match self.installed.get(&key) {
            None | Some(Installed::Checking) => Status::Checking,
            Some(Installed::Missing) => Status::Install(server),
            Some(Installed::Found(_)) => Status::Enable(server, crate::explorer::file_name(&key.1)),
        })
    }

    pub fn diagnostics(&self, path: &Path) -> &[Diagnostic] {
        self.documents
            .get(path)
            .map_or(&[], |document| &document.diagnostics)
    }

    /// How the running server for `path` counts columns, once it is ready.
    pub fn encoding(&self, path: &Path) -> Option<Encoding> {
        let running = self.running.get(&self.documents.get(path)?.key)?;
        running.ready().map(|_| running.encoding)
    }

    /// Sends a request about `path` to its ready server; the reply arrives on the receiver.
    pub fn request(
        &mut self,
        path: &Path,
        method: &str,
        params: Value,
    ) -> Option<oneshot::Receiver<Result<Value, String>>> {
        let key = &self.documents.get(path)?.key;
        let running = self.running.get_mut(key)?;
        if !matches!(running.state, State::Ready) {
            return None;
        }
        let id = running.client.as_mut()?.request(method, params);
        let (reply, receiver) = oneshot::channel();
        running.replies.insert(id, reply);
        Some(receiver)
    }

    /// The footer's Enable: remembered for the project, and the server starts.
    pub fn enable(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((key, server, _)) = key_for(path) else {
            return;
        };
        self.enabled.insert(key.clone());
        save_enabled(&self.enabled);
        // Look again from inside the project, now that the user trusts it; the server
        // starts once found.
        self.installed.remove(&key);
        self.check_installed(server, key, cx);
    }

    pub fn disable(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((key, _, _)) = key_for(path) else {
            return;
        };
        self.enabled.remove(&key);
        save_enabled(&self.enabled);
        self.running.remove(&key);
        self.clear_diagnostics(&key);
        cx.emit(LspEvent::Updated);
    }

    pub fn restart(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((key, server, _)) = key_for(path) else {
            return;
        };
        self.running.remove(&key);
        self.clear_diagnostics(&key);
        self.start(server, key, cx);
    }

    /// The footer's Install runs the server's install command in a terminal; meanwhile the
    /// server is looked for every few seconds, for up to ten minutes.
    pub fn watch_install(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((key, server, _)) = key_for(path) else {
            return;
        };
        self._recheck = Some(cx.spawn(async move |servers, cx| {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(600) {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let found = servers.update(cx, |servers, cx| {
                    let installed = servers.installed.get(&key);
                    let found = matches!(installed, Some(Installed::Found(_)));
                    if !found && !matches!(installed, Some(Installed::Checking)) {
                        servers.installed.remove(&key);
                        servers.check_installed(server, key.clone(), cx);
                    }
                    found
                });
                if !matches!(found, Ok(false)) {
                    break;
                }
            }
        }));
    }

    fn clear_diagnostics(&mut self, key: &Key) {
        for document in self.documents.values_mut() {
            if &document.key == key {
                document.diagnostics.clear();
            }
        }
    }

    /// Looks for the server on the login shell's PATH once per project, off the UI thread.
    fn check_installed(&mut self, server: &'static Server, key: Key, cx: &mut Context<Self>) {
        if self.installed.contains_key(&key) {
            return;
        }
        self.installed.insert(key.clone(), Installed::Checking);
        let trusted = self.enabled.contains(&key);
        cx.spawn(async move |servers, cx| {
            let root = match trusted {
                true => key.1.clone(),
                false => crate::files::home_dir().unwrap_or_else(std::env::temp_dir),
            };
            let found = cx
                .background_executor()
                .spawn(async move { find_server(server, &root) })
                .await;
            servers
                .update(cx, |servers, cx| {
                    let installed = match found {
                        Some(binary) => Installed::Found(binary),
                        None => Installed::Missing,
                    };
                    let found = matches!(installed, Installed::Found(_));
                    servers.installed.insert(key.clone(), installed);
                    let open = servers
                        .documents
                        .values()
                        .any(|document| document.key == key);
                    if found && open && servers.enabled.contains(&key) {
                        servers.start(server, key, cx);
                    }
                    cx.emit(LspEvent::Updated);
                })
                .ok();
        })
        .detach();
    }

    fn start(&mut self, server: &'static Server, key: Key, cx: &mut Context<Self>) {
        if self.running.contains_key(&key) {
            return;
        }
        let Some(Installed::Found(binary)) = self.installed.get(&key) else {
            return;
        };
        let log = std::env::temp_dir().join(format!("bareframe-{}.log", server.binary));
        let (mut client, messages) =
            match Client::spawn(binary, server.args, &key.1, login_path(), &log) {
                Ok(started) => started,
                Err(error) => {
                    self.running
                        .insert(key, failed(format!("could not start: {error}")));
                    cx.emit(LspEvent::Updated);
                    return;
                }
            };
        let root_uri = lsp::path_to_uri(&key.1);
        let name = crate::explorer::file_name(&key.1);
        let id = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {"name": "bareframe"},
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": name}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-8", "utf-16"]},
                    "window": {"workDoneProgress": true},
                    "workspace": {"workspaceFolders": true, "configuration": true},
                    "textDocument": {
                        "synchronization": {"didSave": true},
                        "hover": {"contentFormat": ["plaintext", "markdown"]},
                        "definition": {"linkSupport": true},
                        "publishDiagnostics": {"relatedInformation": false},
                    },
                },
            }),
        );
        let (reply, initialized) = oneshot::channel();
        let mut running = Running {
            client: Some(client),
            state: State::Starting,
            encoding: Encoding::Utf16,
            progress: Vec::new(),
            replies: HashMap::from([(id, reply)]),
            _messages: Some(self.listen(key.clone(), messages, cx)),
        };
        running.progress.push((String::new(), "starting".into()));
        self.running.insert(key.clone(), running);
        cx.spawn(async move |servers, cx| {
            let result = initialized
                .await
                .unwrap_or_else(|_| Err("no answer".into()));
            servers
                .update(cx, |servers, cx| servers.initialized(&key, result, cx))
                .ok();
        })
        .detach();
        cx.emit(LspEvent::Updated);
    }

    fn listen(
        &self,
        key: Key,
        mut messages: mpsc::UnboundedReceiver<Value>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |servers, cx| {
            while let Some(message) = messages.next().await {
                if servers
                    .update(cx, |servers, cx| servers.handle(&key, message, cx))
                    .is_err()
                {
                    return;
                }
            }
            // The output ended: the process exited or crashed. Warp restarts only by hand.
            servers
                .update(cx, |servers, cx| {
                    if let Some(running) = servers.running.get_mut(&key) {
                        running.state = State::Failed("stopped".into());
                        running.client = None;
                        running.progress.clear();
                    }
                    servers.clear_diagnostics(&key);
                    cx.emit(LspEvent::Updated);
                })
                .ok();
        })
    }

    fn initialized(&mut self, key: &Key, result: Result<Value, String>, cx: &mut Context<Self>) {
        let Some(running) = self.running.get_mut(key) else {
            return;
        };
        running.progress.clear();
        match result {
            Ok(reply) => {
                if reply["capabilities"]["positionEncoding"] == "utf-8" {
                    running.encoding = Encoding::Utf8;
                }
                running.state = State::Ready;
                if let Some(client) = &running.client {
                    client.notify("initialized", json!({}));
                    for (path, document) in &self.documents {
                        if &document.key == key {
                            did_open(client, path, document);
                        }
                    }
                }
            }
            Err(error) => running.state = State::Failed(error),
        }
        cx.emit(LspEvent::Updated);
    }

    fn handle(&mut self, key: &Key, message: Value, cx: &mut Context<Self>) {
        let Some(running) = self.running.get_mut(key) else {
            return;
        };
        let method = message["method"].as_str();
        match (method, message.get("id")) {
            // A reply to one of our requests.
            (None, Some(id)) => {
                if let Some(reply) = id.as_i64().and_then(|id| running.replies.remove(&id)) {
                    let result = match message.get("error") {
                        Some(error) => Err(error["message"].as_str().unwrap_or("error").into()),
                        None => Ok(message["result"].clone()),
                    };
                    let _ = reply.send(result);
                }
            }
            // The server asks something; it may wait for the answer before going on.
            (Some(method), Some(id)) => {
                let Some(client) = &running.client else {
                    return;
                };
                let id = id.clone();
                match method {
                    "workspace/configuration" => {
                        let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                        client.respond(id, Value::Array(vec![Value::Null; items]));
                    }
                    "workspace/workspaceFolders" => {
                        let uri = lsp::path_to_uri(&key.1);
                        let name = crate::explorer::file_name(&key.1);
                        client.respond(id, json!([{"uri": uri, "name": name}]));
                    }
                    "window/workDoneProgress/create"
                    | "client/registerCapability"
                    | "client/unregisterCapability"
                    | "window/showMessageRequest" => client.respond(id, Value::Null),
                    _ => client.respond_error(id, -32601, "not supported by bareframe"),
                }
            }
            (Some("$/progress"), None) => {
                let params = &message["params"];
                let token = params["token"].to_string();
                let value = &params["value"];
                running.progress.retain(|(known, _)| known != &token);
                if value["kind"] != "end" {
                    let text = [&value["title"], &value["message"]]
                        .iter()
                        .filter_map(|part| part.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    running.progress.push((token, text));
                }
                cx.emit(LspEvent::Updated);
            }
            (Some("textDocument/publishDiagnostics"), None) => {
                let params = &message["params"];
                let Some(path) = params["uri"].as_str().and_then(lsp::uri_to_path) else {
                    return;
                };
                if let Some(document) = self.documents.get_mut(&path) {
                    document.diagnostics = params["diagnostics"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(diagnostic)
                        .collect();
                    cx.emit(LspEvent::Updated);
                }
            }
            _ => {}
        }
    }
}

fn did_open(client: &Client, path: &Path, document: &Document) {
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": lsp::path_to_uri(path),
            "languageId": document.language,
            "version": document.version,
            "text": document.text,
        }}),
    );
}

fn failed(reason: String) -> Running {
    Running {
        client: None,
        state: State::Failed(reason),
        encoding: Encoding::Utf16,
        progress: Vec::new(),
        replies: HashMap::new(),
        _messages: None,
    }
}

/// Errors and warnings only; a missing severity counts as an error.
fn diagnostic(value: &Value) -> Option<Diagnostic> {
    let severity = value["severity"].as_u64().unwrap_or(1);
    if severity > 2 {
        return None;
    }
    let point = |at: &Value| {
        Some((
            at["line"].as_u64()? as u32,
            at["character"].as_u64()? as u32,
        ))
    };
    Some(Diagnostic {
        start: point(&value["range"]["start"])?,
        end: point(&value["range"]["end"])?,
        error: severity == 1,
        message: value["message"].as_str()?.to_owned(),
    })
}

/// The server's executable on the login shell's PATH, if it works when run from `dir`.
fn find_server(server: &Server, dir: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = login_path()?;
    let binary = std::env::split_paths(path)
        .map(|dir| dir.join(server.binary))
        .find(|file| {
            file.metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })?;
    let Some(probe) = server.probe else {
        return Some(binary);
    };
    Command::new(&binary)
        .args(probe)
        .current_dir(dir)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        .then_some(binary)
}

/// PATH as the user's login shell sets it, so servers in `~/.cargo/bin` or npm's global
/// folder are found though the app was started from the Dock. Asked once; a shell that
/// doesn't answer within five seconds leaves the app's own PATH.
fn login_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        const MARK: &str = "__bareframe_path__";
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let shell_path = Command::new(shell)
            .args([
                "-l",
                "-i",
                "-c",
                &format!("printf '\\n{MARK}%s\\n' \"$PATH\""),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .and_then(|mut child| {
                let started = Instant::now();
                while matches!(child.try_wait(), Ok(None)) {
                    if started.elapsed() > Duration::from_secs(5) {
                        let _ = child.kill();
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let output = child.wait_with_output().ok()?;
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .find_map(|line| line.strip_prefix(MARK).map(str::to_owned))
            });
        shell_path.or_else(|| std::env::var("PATH").ok())
    })
    .as_deref()
}

fn enabled_file() -> Option<PathBuf> {
    Some(crate::files::config_file()?.with_file_name("language-servers"))
}

/// `binary<TAB>root` per line.
fn load_enabled() -> HashSet<Key> {
    let Some(file) = enabled_file() else {
        return HashSet::new();
    };
    let text = crate::files::read_optional(&file)
        .inspect_err(|error| eprintln!("bareframe: {}: {error}", file.display()))
        .ok()
        .flatten()
        .unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (binary, root) = line.split_once('\t')?;
            let server = lsp::SERVERS.iter().find(|server| server.binary == binary)?;
            Some((server.binary, PathBuf::from(root)))
        })
        .collect()
}

fn save_enabled(enabled: &HashSet<Key>) {
    let Some(file) = enabled_file() else {
        return;
    };
    // A line break in a folder name would start a line of its own and enable a project
    // the user never chose; such a project stays enabled for this run only.
    let mut lines: Vec<String> = enabled
        .iter()
        .filter(|(_, root)| !root.to_string_lossy().chars().any(char::is_control))
        .map(|(binary, root)| format!("{binary}\t{}\n", root.display()))
        .collect();
    lines.sort();
    if let Err(error) = crate::files::write_atomically(&file, &lines.concat()) {
        eprintln!("bareframe: {}: {error}", file.display());
    }
}
