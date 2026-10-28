//! `bareframe-browser`: bareframe's browser tabs for a program in one of its terminals, as
//! commands (scripts) or as an MCP server over stdio (`mcp`, for Claude Code, Codex and
//! other agents). It talks to the app over the socket named in `BAREFRAME_BROWSER_ENDPOINT`,
//! which terminals get while Settings › Features › Browser › "Let agents in terminals
//! control browser tabs" is on; it never starts a browser itself.
// Shared with the app, which also uses the names only it needs.
#[allow(dead_code)]
#[path = "../browser/protocol.rs"]
mod protocol;

use base64::Engine;
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    os::unix::net::UnixStream,
    process::ExitCode,
};

const USAGE: &str = "usage: bareframe-browser tabs
       bareframe-browser open <url>
       bareframe-browser navigate <tab> <url>
       bareframe-browser snapshot <tab>
       bareframe-browser screenshot <tab> [<file.png>]
       bareframe-browser click <tab> <ref> [<document>]
       bareframe-browser fill <tab> <ref> <text> [<document>]
       bareframe-browser press <tab> <key>
       bareframe-browser wait <tab> <text> [<timeout_ms>]
       bareframe-browser diagnostics <tab> [<cursor>]
       bareframe-browser release <tab>
       bareframe-browser close <tab>
       bareframe-browser mcp        (an MCP server over stdio)
       bareframe-browser hook       (Claude Code's PreToolUse hook: asks before a first use)
       bareframe-browser setup      (how to register both with Claude Code)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["mcp"] => return mcp::serve(),
        ["hook"] => return hook(),
        ["setup"] => return setup(),
        _ => {}
    }
    let number = |text: &str| text.parse::<i64>().ok().map(Value::from);
    let (method, params, file) = match args.as_slice() {
        ["tabs"] => ("tabs", json!({}), None),
        ["open", url] => ("open", json!({ "url": url }), None),
        ["navigate", tab, url] => ("navigate", json!({ "tab": tab, "url": url }), None),
        ["snapshot", tab] => ("snapshot", json!({ "tab": tab }), None),
        ["screenshot", tab] => (
            "screenshot",
            json!({ "tab": tab }),
            Some(format!("{tab}.png")),
        ),
        ["screenshot", tab, file] => ("screenshot", json!({ "tab": tab }), Some(file.to_string())),
        ["click", tab, node, rest @ ..] if rest.len() <= 1 => (
            "click",
            json!({ "tab": tab, "ref": number(node), "document": rest.first().and_then(|d| number(d)) }),
            None,
        ),
        ["fill", tab, node, text, rest @ ..] if rest.len() <= 1 => (
            "fill",
            json!({ "tab": tab, "ref": number(node), "text": text,
                    "document": rest.first().and_then(|d| number(d)) }),
            None,
        ),
        ["press", tab, key] => ("press_key", json!({ "tab": tab, "key": key }), None),
        ["wait", tab, text, rest @ ..] if rest.len() <= 1 => (
            "wait_for",
            json!({ "tab": tab, "text": text, "timeout_ms": rest.first().and_then(|t| number(t)) }),
            None,
        ),
        ["diagnostics", tab, rest @ ..] if rest.len() <= 1 => (
            "diagnostics",
            json!({ "tab": tab, "since": rest.first().and_then(|c| number(c)) }),
            None,
        ),
        ["release", tab] => ("release", json!({ "tab": tab }), None),
        ["close", tab] => ("close", json!({ "tab": tab }), None),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = request(method, params).and_then(|result| match file {
        Some(file) => save(&result, &file),
        None => Ok(result),
    });
    match result {
        // A snapshot reads best as the outline itself.
        Ok(result) if method == "snapshot" => {
            println!(
                "# {} document {}",
                result["tab"].as_str().unwrap_or_default(),
                result["document"]
            );
            println!("{}", result["text"].as_str().unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(result) => {
            println!("{result:#}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bareframe-browser: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The hook that asks the user before an agent first uses a tab from this terminal, naming
/// the page. For Claude Code it is a PreToolUse hook: it asks, and leaves later uses and
/// calls that touch no tab to Claude Code's own rules (no output). Codex can't ask from
/// there, so it asks before every call of ours (see `setup`) and this is its
/// PermissionRequest hook: it allows a tab in use already and letting one go, and leaves the
/// rest to Codex's question, saying which page it is. When the app can't say the tab is in
/// use already, it asks too.
fn hook() -> ExitCode {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let Ok(input) = serde_json::from_str::<Value>(&input) else {
        return ExitCode::SUCCESS;
    };
    let codex = input["hook_event_name"] == "PermissionRequest";
    let allow = || {
        if codex {
            let allow = json!({ "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow" },
            } });
            println!("{allow}");
        }
        ExitCode::SUCCESS
    };
    let tool = input["tool_name"].as_str().unwrap_or_default();
    let method = tool.rsplit("__").next().unwrap_or(tool);
    let verb = match method.strip_prefix("browser_").unwrap_or(method) {
        "snapshot" | "screenshot" | "diagnostics" => "read",
        "navigate" => "load another page in",
        "click" | "fill" | "press_key" => "click and type in",
        "wait_for" => "watch",
        "close" => "close",
        "release" => return allow(),
        // A new tab shares the sign-ins of the user's own tabs: Codex asks, as Claude Code
        // does for a tool it has not been allowed.
        "open" if codex => {
            let url = input["tool_input"]["url"].as_str().unwrap_or_default();
            let note = format!(
                "The agent wants to open {url} in a new bareframe browser tab, signed in as \
                 your other tabs are, and use it."
            );
            println!("{}", json!({ "systemMessage": note }));
            return ExitCode::SUCCESS;
        }
        _ => "use",
    };
    let Some(tab) = input["tool_input"]["tab"].as_str() else {
        return ExitCode::SUCCESS;
    };
    let status = request("granted", json!({ "tab": tab })).unwrap_or_default();
    if status["granted"].as_bool() == Some(true) {
        return allow();
    }
    let (title, url) = (
        status["title"].as_str().unwrap_or_default(),
        status["url"].as_str().unwrap_or(tab),
    );
    let page = if title.is_empty() || title == url {
        url.to_owned()
    } else {
        format!("\"{title}\" ({url})")
    };
    let reason = format!(
        "The agent wants to {verb} the bareframe browser tab {page}. Once allowed, it can \
         read, click and type there until it releases the tab; ask it to."
    );
    let question = if codex {
        // No decision: Codex asks. Allowing for good would skip asking about other tabs.
        json!({ "systemMessage": format!("{reason} Allow it once, not for the session or always.") })
    } else {
        json!({ "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": reason,
        } })
    };
    println!("{question}");
    ExitCode::SUCCESS
}

/// Prints how to register the MCP server and the hook with Claude Code and with Codex, with
/// this executable's own path. Nothing is changed here; the user adds them.
fn setup() -> ExitCode {
    let path = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "bareframe-browser".into());
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    let hooks = |event: &str, matcher: &str, command: String| {
        json!({ "hooks": { event: [{
            "matcher": matcher,
            "hooks": [{ "type": "command", "command": command }],
        }] } })
    };
    let find = FIND_VAR;
    let (claude, codex) = (
        hooks(
            "PreToolUse",
            "mcp__bareframe-browser__.*",
            format!("{quoted} hook"),
        ),
        hooks(
            "PermissionRequest",
            "^mcp__bareframe-browser__",
            format!("{find}=1 {quoted} hook"),
        ),
    );
    // A JSON string is a TOML string too.
    let path = Value::from(path);
    println!(
        "Claude Code\n\n\
         1. Register the MCP server for every project:\n\n   \
         claude mcp add --scope user bareframe-browser -- {quoted} mcp\n\n\
         2. Have Claude Code ask before an agent first uses a browser tab: add this to\n   \
         ~/.claude/settings.json (merged with any \"hooks\" already there):\n\n{claude:#}\n\n\
         Codex\n\n\
         1. Add this to ~/.codex/config.toml. Codex then asks before each browser call. The\n   \
         server gets the terminal's socket and id; Codex's shared server (its daemon) runs\n   \
         outside bareframe's terminals and gets neither, so it finds the socket itself and\n   \
         the tabs one Codex is allowed are allowed to all. `codex --no-daemon` in a\n   \
         bareframe terminal keeps them to that terminal.\n\n\
         [mcp_servers.bareframe-browser]\n\
         command = {path}\n\
         args = [\"mcp\"]\n\
         env_vars = [\"{endpoint}\", \"{terminal}\"]\n\
         env = {{ {find} = \"1\" }}\n\
         default_tools_approval_mode = \"prompt\"\n\n\
         2. Have the hook answer for tabs in use already: add this to ~/.codex/hooks.json\n   \
         (merged with any \"hooks\" there), then trust it when Codex asks (or in /hooks).\n   \
         When Codex asks about a tab, allow it once: for the session or always would skip\n   \
         asking about other tabs.\n\n{codex:#}\n\n\
         Then turn on Settings › Features › Browser › \"Let agents in terminals control\n\
         browser tabs\" in bareframe, and start the agent in a terminal opened after that.",
        endpoint = protocol::ENDPOINT_VAR,
        terminal = protocol::TERMINAL_VAR,
    );
    ExitCode::SUCCESS
}

/// Set in an agent's config for a server started outside bareframe's terminals, which gets
/// none of their variables (Codex's shared one): it looks for the socket instead.
const FIND_VAR: &str = "BAREFRAME_BROWSER_FIND";

const NOWHERE: &str = "no bareframe here: run this in a bareframe terminal opened after \
    turning on Settings › Features › Browser › \"Let agents in terminals control browser tabs\"";

/// The socket of the one bareframe that answers, for a server started outside its terminals
/// (see `FIND_VAR`). It can't tell which terminal its agent runs in, so the tabs it
/// is allowed are those of every agent started that way.
fn find() -> Result<String, String> {
    let entries = std::fs::read_dir(std::env::temp_dir()).map_err(|error| error.to_string())?;
    let live: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(protocol::SOCKET_PREFIX) && name.ends_with(".sock"))
        .map(|name| std::env::temp_dir().join(name).display().to_string())
        // Left behind by a bareframe that crashed, or a file of the same name.
        .filter(|path| UnixStream::connect(path).is_ok())
        .collect();
    match live.as_slice() {
        [endpoint] => Ok(endpoint.clone()),
        [] => Err(NOWHERE.to_owned()),
        _ => Err(
            "several bareframes let agents use their tabs: start the agent in a \
                  bareframe terminal without a shared server (codex --no-daemon)"
                .to_owned(),
        ),
    }
}

/// One request to the app and its result; the terminal's id goes with it, since what the
/// user allows is that terminal using a tab.
fn request(method: &str, mut params: Value) -> Result<Value, String> {
    let endpoint = match std::env::var(protocol::ENDPOINT_VAR) {
        Ok(endpoint) => endpoint,
        Err(_) if std::env::var_os(FIND_VAR).is_some() => find()?,
        Err(_) => return Err(NOWHERE.to_owned()),
    };
    if let Ok(terminal) = std::env::var(protocol::TERMINAL_VAR) {
        params["terminal"] = Value::from(terminal);
    }
    let stream = UnixStream::connect(&endpoint)
        .map_err(|error| format!("{endpoint}: {error} (is that bareframe still running?)"))?;
    let mut writer = stream.try_clone().map_err(|error| error.to_string())?;
    let message = json!({ "id": 1, "method": method, "params": params });
    writeln!(writer, "{message}").map_err(|error| error.to_string())?;
    let line = protocol::read_line(&mut BufReader::new(stream))
        .map_err(|error| error.to_string())?
        .ok_or("bareframe closed the connection")?;
    let mut reply: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
    if let Some(error) = reply.get("error") {
        return Err(format!(
            "{}: {}",
            error["code"].as_str().unwrap_or("ERROR"),
            error["message"].as_str().unwrap_or_default()
        ));
    }
    Ok(reply["result"].take())
}

/// Writes a screenshot's PNG to `file`; the result says where, instead of carrying it.
fn save(result: &Value, file: &str) -> Result<Value, String> {
    let png = base64::engine::general_purpose::STANDARD
        .decode(result["data"].as_str().unwrap_or_default())
        .map_err(|error| format!("bad screenshot: {error}"))?;
    std::fs::write(file, png).map_err(|error| format!("{file}: {error}"))?;
    Ok(json!({ "tab": result["tab"], "document": result["document"], "path": file }))
}

/// The same requests as MCP tools: JSON-RPC, one message a line on stdin and stdout.
mod mcp {
    use super::{protocol, request};
    use serde_json::{Value, json};
    use std::{io::Write, process::ExitCode};

    const INSTRUCTIONS: &str = "These tools drive the browser tabs of the bareframe window \
        this agent runs in: the same pages the user sees. Start with browser_tabs, or \
        browser_open for a tab of your own. browser_snapshot gives the page as an outline \
        whose elements carry [ref=N]; pass a ref and the snapshot's document number to \
        browser_click or browser_fill. When the page changes to another document, refs go \
        stale: take a new snapshot. Before you first use a tab from this terminal, the user \
        is asked to allow it. The user manages your tabs through you: when you are done, \
        close the tabs you opened and release the ones you were allowed, unless the user \
        wants to see the result (\"leave it open\"); then leave it and say which tab it is. \
        Release or close a tab whenever the user asks you to stop using it. Check outcomes \
        with browser_wait_for or a new snapshot rather than assuming an action worked, and \
        with browser_diagnostics: take its cursor before an action and read what came after. \
        OUTCOME_UNKNOWN means the input may have reached the page: look before clicking or \
        submitting again.";

    pub fn serve() -> ExitCode {
        let mut stdin = std::io::stdin().lock();
        let mut stdout = std::io::stdout().lock();
        while let Ok(Some(line)) = protocol::read_line(&mut stdin) {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                let reply = json!({ "jsonrpc": "2.0", "id": null,
                    "error": { "code": -32700, "message": "parse error" } });
                let _ = writeln!(stdout, "{reply}");
                continue;
            };
            // Notifications carry no id and get no answer.
            let Some(id) = message.get("id").cloned() else {
                continue;
            };
            let reply = match answer(&message) {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, text)) => json!({ "jsonrpc": "2.0", "id": id,
                    "error": { "code": code, "message": text } }),
            };
            if writeln!(stdout, "{reply}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
        ExitCode::SUCCESS
    }

    fn answer(message: &Value) -> Result<Value, (i64, String)> {
        let params = &message["params"];
        match message["method"].as_str().unwrap_or_default() {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "bareframe-browser", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(call(
                params["name"].as_str().unwrap_or_default(),
                &params["arguments"],
            )),
            method => Err((-32601, format!("no method {method}"))),
        }
    }

    fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
        })
    }

    fn tools() -> Vec<Value> {
        let tab = json!({ "type": "string", "description": "A tab id from browser_tabs or browser_open" });
        let node =
            json!({ "type": "integer", "description": "An element's ref from browser_snapshot" });
        let document = json!({ "type": "integer",
            "description": "The document number of the snapshot the ref came from" });
        vec![
            tool(
                "browser_tabs",
                "List the browser tabs: id, title, URL, whether ready, shown and loading.",
                json!({}),
                &[],
            ),
            tool(
                "browser_open",
                "Open a URL in a new browser tab of your own, in front, and wait for it to load.",
                json!({ "url": { "type": "string" } }),
                &["url"],
            ),
            tool(
                "browser_navigate",
                "Load a URL in a tab and wait for it to finish loading.",
                json!({ "tab": tab, "url": { "type": "string" } }),
                &["tab", "url"],
            ),
            tool(
                "browser_snapshot",
                "The page as an accessibility outline, one element a line, with [ref=N] to act on.",
                json!({ "tab": tab }),
                &["tab"],
            ),
            tool(
                "browser_screenshot",
                "A PNG of what the tab shows.",
                json!({ "tab": tab }),
                &["tab"],
            ),
            tool(
                "browser_click",
                "Click an element as a mouse would, at its middle; fails if something covers it.",
                json!({ "tab": tab, "ref": node, "document": document }),
                &["tab", "ref", "document"],
            ),
            tool(
                "browser_fill",
                "Replace the text of a field with new text, typed in as input.",
                json!({ "tab": tab, "ref": node, "text": { "type": "string" }, "document": document }),
                &["tab", "ref", "text", "document"],
            ),
            tool(
                "browser_press_key",
                "Press a key: Enter, Tab, Escape, Backspace, Delete, arrows, Home, End, PageUp, PageDown, or one character.",
                json!({ "tab": tab, "key": { "type": "string" } }),
                &["tab", "key"],
            ),
            tool(
                "browser_wait_for",
                "Wait until the page shows some text (default up to 15 s).",
                json!({ "tab": tab, "text": { "type": "string" }, "timeout_ms": { "type": "integer" } }),
                &["tab", "text"],
            ),
            tool(
                "browser_diagnostics",
                "What went wrong on the page while agents may use the tab: console errors and \
                 warnings, uncaught exceptions, HTTP errors (status 400 and up) and failed \
                 requests. Pass the cursor of an earlier call as since to get only what came \
                 after it.",
                json!({ "tab": tab, "since": { "type": "integer" } }),
                &["tab"],
            ),
            tool(
                "browser_release",
                "Stop using a tab, leaving it open; using it again asks the user again.",
                json!({ "tab": tab }),
                &["tab"],
            ),
            tool(
                "browser_close",
                "Close a tab you opened with browser_open.",
                json!({ "tab": tab }),
                &["tab"],
            ),
        ]
    }

    /// A tool's result as MCP content: text, the snapshot's outline, or the screenshot.
    fn call(name: &str, arguments: &Value) -> Value {
        let method = name.strip_prefix("browser_").unwrap_or(name);
        let arguments = if arguments.is_object() {
            arguments.clone()
        } else {
            json!({})
        };
        match request(method, arguments) {
            Ok(result) if method == "snapshot" => {
                let mut text = format!(
                    "{} document {}\n{}",
                    result["tab"].as_str().unwrap_or_default(),
                    result["document"],
                    result["text"].as_str().unwrap_or_default()
                );
                if result["truncated"].as_bool() == Some(true) {
                    text.push_str("\n(cut short: the page has more)");
                }
                json!({ "content": [{ "type": "text", "text": text }] })
            }
            Ok(result) if method == "screenshot" => json!({ "content": [
                { "type": "image", "data": result["data"], "mimeType": "image/png" },
                { "type": "text", "text": format!("{} document {}",
                    result["tab"].as_str().unwrap_or_default(), result["document"]) },
            ] }),
            Ok(result) => json!({ "content": [{ "type": "text", "text": result.to_string() }] }),
            Err(error) => {
                json!({ "content": [{ "type": "text", "text": error }], "isError": true })
            }
        }
    }
}
