//! One language server process: a thread writing framed messages to its stdin, and one
//! reading its stdout back into JSON values. Pairing replies with requests is the caller's.
use bareframe_core::lsp::{frame, read_message};
use futures::channel::mpsc;
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc as queue,
    time::Duration,
};

pub struct Client {
    child: Option<Child>,
    writer: queue::Sender<Vec<u8>>,
    next_id: i64,
}

impl Client {
    /// Starts `binary` in `root` with `path` as its PATH. Its stderr goes to `log`.
    pub fn spawn(
        binary: &Path,
        args: &[&str],
        root: &Path,
        path: Option<&str>,
        log: &Path,
    ) -> std::io::Result<(Self, mpsc::UnboundedReceiver<Value>)> {
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(log).map_or_else(|_| Stdio::null(), Stdio::from));
        if let Some(path) = path {
            command.env("PATH", path);
        }
        let mut child = command.spawn()?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(std::io::Error::other("no pipes to the language server"));
        };
        let (writer, outgoing) = queue::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for message in outgoing {
                if stdin
                    .write_all(&message)
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let (incoming, receiver) = mpsc::unbounded();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(body)) = read_message(&mut reader) {
                match serde_json::from_str::<Value>(&body) {
                    Ok(message) => {
                        if incoming.unbounded_send(message).is_err() {
                            break;
                        }
                    }
                    Err(error) => eprintln!("bareframe: language server sent bad JSON: {error}"),
                }
            }
        });
        let client = Self {
            child: Some(child),
            writer,
            next_id: 0,
        };
        Ok((client, receiver))
    }

    /// Sends a request and returns its id.
    pub fn request(&mut self, method: &str, params: Value) -> i64 {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn respond(&self, id: Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    fn send(&self, message: Value) {
        // A closed queue means the process is gone; the reader's end reports that.
        let _ = self.writer.send(frame(&message.to_string()));
    }
}

impl Drop for Client {
    /// Asks the server to shut down and exit, and kills it if it is still there shortly after.
    fn drop(&mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    /// Talks to a real server when one is installed: clangd ships with Xcode's tools.
    #[test]
    fn a_real_server_initializes_and_reports_diagnostics() {
        let clangd = Path::new("/usr/bin/clangd");
        if !clangd.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("bareframe-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bad.c");
        let log = dir.join("clangd.log");
        let (mut client, mut messages) = Client::spawn(clangd, &[], &dir, None, &log).unwrap();
        let id = client.request(
            "initialize",
            json!({"processId": null, "rootUri": bareframe_core::lsp::path_to_uri(&dir),
                   "capabilities": {"general": {"positionEncodings": ["utf-8", "utf-16"]}}}),
        );
        let next = |messages: &mut futures::channel::mpsc::UnboundedReceiver<Value>| {
            futures::executor::block_on(messages.next()).expect("server closed its output")
        };
        let reply = loop {
            let message = next(&mut messages);
            if message["id"] == id {
                break message;
            }
        };
        assert!(reply["result"]["capabilities"].is_object(), "{reply}");
        client.notify("initialized", json!({}));
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": bareframe_core::lsp::path_to_uri(&file),
                "languageId": "c", "version": 1, "text": "int main(void) { return x; }\n"}}),
        );
        let diagnostics = loop {
            let message = next(&mut messages);
            if message["method"] == "textDocument/publishDiagnostics" {
                break message["params"]["diagnostics"].clone();
            }
        };
        let first = &diagnostics[0];
        assert_eq!(first["severity"], 1, "{diagnostics}");
        assert_eq!(first["range"]["start"]["character"], 24, "{diagnostics}");
        drop(client);
        std::fs::remove_dir_all(&dir).ok();
    }
}
