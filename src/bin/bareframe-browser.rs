//! `bareframe-browser`: lists and reads bareframe's browser tabs for a program in one of its
//! terminals (an agent, a test script). It talks to the app over the socket named in
//! `BAREFRAME_BROWSER_ENDPOINT`, which terminals get while Settings › Features › Browser ›
//! "Let agents in terminals control browser tabs" is on; it never starts a browser itself.
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
       bareframe-browser snapshot <tab>
       bareframe-browser screenshot <tab> [<file.png>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (method, tab, file) = match args.as_slice() {
        ["tabs"] => ("tabs", None, None),
        ["snapshot", tab] => ("snapshot", Some(*tab), None),
        ["screenshot", tab] => ("screenshot", Some(*tab), Some(format!("{tab}.png"))),
        ["screenshot", tab, file] => ("screenshot", Some(*tab), Some((*file).to_owned())),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let params = tab.map_or_else(|| json!({}), |tab| json!({ "tab": tab }));
    let result = request(method, params).and_then(|result| match file {
        Some(file) => save(&result, &file),
        None => Ok(result),
    });
    match result {
        // A snapshot reads best as the outline itself.
        Ok(result) if method == "snapshot" => {
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

/// One request to the app and its result.
fn request(method: &str, params: Value) -> Result<Value, String> {
    let endpoint = std::env::var(protocol::ENDPOINT_VAR).map_err(|_| {
        "no bareframe here: run this in a bareframe terminal opened after turning on \
         Settings › Features › Browser › \"Let agents in terminals control browser tabs\""
            .to_owned()
    })?;
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
