//! The app's language servers, run as Warp runs them: one per server and project root,
//! started once the user enables that server for the project, shared by every editor on a
//! file inside it, and stopped when its last file closes. Enabled projects are remembered
//! beside the settings file (`store`).
mod client;
mod install;
pub mod python;
mod server;
mod store;

use bareframe_core::lsp::{self, Server};
use futures::channel::oneshot;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Subscription, Task};
use install::{Installed, find_interpreters};
use python::Interpreter;
use serde_json::{Value, json};
use server::{Running, did_open};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

const PYRIGHT: &str = "pyright-langserver";

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
        /// The Python interpreter pyright uses.
        python: Option<Interpreter>,
    },
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
    /// Per server binary: the globally installed one is used for every project.
    installed: HashMap<&'static str, Installed>,
    running: HashMap<Key, Running>,
    documents: HashMap<PathBuf, Document>,
    /// The interpreter picked for each project; others get `python::default`.
    python: HashMap<PathBuf, PathBuf>,
    /// The interpreters found for each project, looked for when its first Python file
    /// opens, when the list is opened and on restart; reading folders on every redraw
    /// would stall the window.
    interpreters: HashMap<PathBuf, Vec<Interpreter>>,
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
        enabled: store::load_enabled(),
        installed: HashMap::new(),
        running: HashMap::new(),
        documents: HashMap::new(),
        python: store::load_pythons(),
        interpreters: HashMap::new(),
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
        if key.0 == PYRIGHT && !self.interpreters.contains_key(&key.1) {
            self.find_pythons(path);
        }
        self.check_installed(server, cx);
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
                python: (key.0 == PYRIGHT)
                    .then(|| self.python_for(&key.1))
                    .flatten(),
            });
        }
        Some(match self.installed.get(server.binary) {
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
        store::save_enabled(&self.enabled);
        self.start(server, key, cx);
    }

    pub fn disable(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((key, _, _)) = key_for(path) else {
            return;
        };
        self.enabled.remove(&key);
        store::save_enabled(&self.enabled);
        self.running.remove(&key);
        self.clear_diagnostics(&key);
        cx.emit(LspEvent::Updated);
    }

    /// The interpreters last found for the project of `path`, and the one in use.
    pub fn interpreters(&self, path: &Path) -> (Vec<Interpreter>, Option<Interpreter>) {
        let root = lsp::project_root(path);
        let found = self.interpreters.get(&root).cloned().unwrap_or_default();
        (found, self.python_for(&root))
    }

    /// Looks for the interpreters of `path`'s project again.
    pub fn find_pythons(&mut self, path: &Path) {
        let root = lsp::project_root(path);
        let found = find_interpreters(&root);
        self.interpreters.insert(root, found);
    }

    /// The footer's pick: remembered for the project; pyright restarts to use it.
    pub fn choose_python(&mut self, path: &Path, python: PathBuf, cx: &mut Context<Self>) {
        self.python.insert(lsp::project_root(path), python);
        store::save_pythons(&self.python);
        self.restart(path, cx);
    }

    fn python_for(&self, root: &Path) -> Option<Interpreter> {
        let found = self.interpreters.get(root).map_or(&[][..], Vec::as_slice);
        let chosen = self
            .python
            .get(root)
            .and_then(|chosen| found.iter().find(|found| &found.python == chosen));
        chosen.or_else(|| python::default(found, root)).cloned()
    }

    pub fn restart(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((key, server, _)) = key_for(path) else {
            return;
        };
        if key.0 == PYRIGHT {
            self.find_pythons(path);
        }
        self.running.remove(&key);
        self.clear_diagnostics(&key);
        self.start(server, key, cx);
    }

    fn clear_diagnostics(&mut self, key: &Key) {
        for document in self.documents.values_mut() {
            if &document.key == key {
                document.diagnostics.clear();
            }
        }
    }
}
