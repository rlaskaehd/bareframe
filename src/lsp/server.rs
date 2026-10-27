//! One running server: starting it with `initialize`, answering what it asks, and taking
//! in its replies, progress and diagnostics.
use super::{
    Diagnostic, Document, Encoding, Key, LanguageServers, LspEvent, PYRIGHT, State,
    client::Client,
    install::{Installed, login_path},
};
use bareframe_core::lsp::{self, Server};
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{Context, Task};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

pub(super) struct Running {
    /// `None` when the process couldn't start; the state says why.
    pub(super) client: Option<Client>,
    pub(super) state: State,
    pub(super) encoding: Encoding,
    /// Work the server reports by progress token, oldest first.
    pub(super) progress: Vec<(String, String)>,
    pub(super) replies: HashMap<i64, oneshot::Sender<Result<Value, String>>>,
    pub(super) _messages: Option<Task<()>>,
}

impl Running {
    pub(super) fn ready(&self) -> Option<&Client> {
        matches!(self.state, State::Ready)
            .then_some(self.client.as_ref())
            .flatten()
    }
}

impl LanguageServers {
    pub(super) fn start(&mut self, server: &'static Server, key: Key, cx: &mut Context<Self>) {
        if self.running.contains_key(&key) {
            return;
        }
        let Some(Installed::Found(binary)) = self.installed.get(server.binary) else {
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
        // pyright asks for the `python` section to learn which interpreter to use.
        let python = (key.0 == PYRIGHT && message["method"] == "workspace/configuration")
            .then(|| self.python_for(&key.1))
            .flatten();
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
                        let answers = message["params"]["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|item| match (&python, item["section"].as_str()) {
                                (Some(python), Some("python")) => {
                                    json!({"pythonPath": python.python})
                                }
                                _ => Value::Null,
                            })
                            .collect();
                        client.respond(id, Value::Array(answers));
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

pub(super) fn did_open(client: &Client, path: &Path, document: &Document) {
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
