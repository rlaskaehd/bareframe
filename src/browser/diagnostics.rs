//! What went wrong on a page while an agent used it, for telling a working page from one
//! that only looks fine: console errors and warnings, uncaught exceptions, HTTP errors and
//! requests that failed outright. Each entry has a number (the cursor); an agent reads the
//! entries after one it saw, so what an action caused shows on its own.
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

/// The most entries kept for a page; older ones go first.
const ENTRIES: usize = 200;
/// The longest text kept from one message.
const TEXT: usize = 1000;
/// Requests in flight remembered for their address, should they fail.
// ponytail: forgets them all past this, add per-document clearing if pages lose addresses
const REQUESTS: usize = 1000;

/// The events `record` reads; the rest need not be parsed.
pub const METHODS: [&str; 6] = [
    "Runtime.consoleAPICalled",
    "Runtime.exceptionThrown",
    "Network.requestWillBeSent",
    "Network.loadingFinished",
    "Network.responseReceived",
    "Network.loadingFailed",
];

#[derive(Default)]
pub struct Log {
    last: u64,
    entries: VecDeque<Value>,
    /// Addresses of requests in flight: a failure names only its request.
    requests: HashMap<String, String>,
    /// No agent may use the page: nothing is noted.
    paused: bool,
}

impl Log {
    /// Forgets what was noted and notes nothing until `resume`, while no agent may use the
    /// page. Cursors go on from where they were, so an old one misses nothing new.
    pub fn pause(&mut self) {
        self.entries.clear();
        self.requests.clear();
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Notes a DevTools event from the `Runtime` or `Network` domain, if it tells of a fault.
    pub fn record(&mut self, method: &str, params: &Value, document: u64) {
        if self.paused {
            return;
        }
        let mut entry = match method {
            "Runtime.consoleAPICalled" => {
                let level = params["type"].as_str().unwrap_or_default();
                if !matches!(level, "error" | "warning" | "assert") {
                    return;
                }
                let text: Vec<String> = params["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|arg| match &arg["value"] {
                        Value::String(text) => text.clone(),
                        Value::Null => arg["description"].as_str().unwrap_or("").to_owned(),
                        value => value.to_string(),
                    })
                    .collect();
                json!({ "kind": "console", "level": level, "text": cut(&text.join(" ")) })
            }
            "Runtime.exceptionThrown" => {
                let details = &params["exceptionDetails"];
                let text = details["exception"]["description"]
                    .as_str()
                    .or(details["text"].as_str())
                    .unwrap_or_default();
                json!({ "kind": "exception", "text": cut(text),
                        "url": plain(details["url"].as_str().unwrap_or_default()),
                        "line": details["lineNumber"] })
            }
            "Network.requestWillBeSent" => {
                if self.requests.len() >= REQUESTS {
                    self.requests.clear();
                }
                if let (Some(id), Some(url)) = (
                    params["requestId"].as_str(),
                    params["request"]["url"].as_str(),
                ) {
                    self.requests.insert(id.to_owned(), plain(url));
                }
                return;
            }
            "Network.loadingFinished" => {
                if let Some(id) = params["requestId"].as_str() {
                    self.requests.remove(id);
                }
                return;
            }
            "Network.responseReceived" => {
                let status = params["response"]["status"].as_u64().unwrap_or_default();
                if status < 400 {
                    return;
                }
                json!({ "kind": "http", "status": status, "resource": params["type"],
                        "url": plain(params["response"]["url"].as_str().unwrap_or_default()) })
            }
            "Network.loadingFailed" => {
                let url = params["requestId"]
                    .as_str()
                    .and_then(|id| self.requests.remove(id));
                // Stopped by the page or a navigation, not a fault.
                if params["canceled"].as_bool() == Some(true) {
                    return;
                }
                json!({ "kind": "network", "error": params["errorText"],
                        "resource": params["type"], "url": url })
            }
            _ => return,
        };
        self.last += 1;
        entry["cursor"] = Value::from(self.last);
        entry["document"] = Value::from(document);
        if self.entries.len() == ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// The entries after `cursor`, the cursor to read from next time, and how many entries
    /// after `cursor` are gone already.
    pub fn since(&self, cursor: u64) -> Value {
        let entries: Vec<&Value> = self
            .entries
            .iter()
            .filter(|entry| entry["cursor"].as_u64() > Some(cursor))
            .collect();
        let oldest = self
            .entries
            .front()
            .and_then(|entry| entry["cursor"].as_u64())
            .unwrap_or(self.last + 1);
        json!({
            "entries": entries,
            "cursor": self.last,
            "missed": oldest.saturating_sub(cursor + 1),
        })
    }
}

fn cut(text: &str) -> String {
    match text.char_indices().nth(TEXT) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// An address without what tends to carry secrets: a user and password, the query, the
/// fragment.
fn plain(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    let url = match url.split_once("://") {
        Some((scheme, rest)) => {
            let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let host = host.rsplit('@').next().unwrap_or(host);
            format!("{scheme}://{host}{path}")
        }
        None => url.to_owned(),
    };
    cut(&url)
}

#[cfg(test)]
mod tests {
    use super::{ENTRIES, Log, plain};
    use serde_json::json;

    #[test]
    fn faults_are_kept_and_read_from_a_cursor() {
        let mut log = Log::default();
        log.record(
            "Runtime.consoleAPICalled",
            &json!({ "type": "log", "args": [{ "value": "fine" }] }),
            1,
        );
        log.record(
            "Runtime.consoleAPICalled",
            &json!({ "type": "error", "args": [{ "value": "boom" }, { "value": 2 }] }),
            1,
        );
        log.record(
            "Network.requestWillBeSent",
            &json!({ "requestId": "7", "request": { "url": "http://u:p@a.b/x?token=1#f" } }),
            1,
        );
        log.record(
            "Network.loadingFailed",
            &json!({ "requestId": "7", "errorText": "net::ERR_CONNECTION_REFUSED" }),
            1,
        );
        log.record(
            "Network.responseReceived",
            &json!({ "type": "Fetch", "response": { "status": 500, "url": "http://a.b/api" } }),
            2,
        );
        log.record(
            "Network.loadingFailed",
            &json!({ "requestId": "8", "canceled": true }),
            2,
        );
        let all = log.since(0);
        assert_eq!(all["cursor"], 3);
        assert_eq!(all["entries"][0]["text"], "boom 2");
        assert_eq!(all["entries"][1]["url"], "http://a.b/x");
        assert_eq!(all["entries"][1]["kind"], "network");
        assert_eq!(all["entries"][2]["status"], 500);
        assert_eq!(all["entries"][2]["document"], 2);
        assert_eq!(log.since(2)["entries"].as_array().unwrap().len(), 1);

        for _ in 0..ENTRIES {
            log.record(
                "Runtime.exceptionThrown",
                &json!({ "exceptionDetails": { "text": "x" } }),
                2,
            );
        }
        assert_eq!(log.since(0)["missed"], 3);
        assert_eq!(log.since(3)["missed"], 0);

        // Released: what happens meanwhile is not kept, and numbering goes on after.
        log.pause();
        let fault = json!({ "exceptionDetails": { "text": "y" } });
        log.record("Runtime.exceptionThrown", &fault, 3);
        assert_eq!(log.since(0)["entries"].as_array().unwrap().len(), 0);
        log.resume();
        log.record("Runtime.exceptionThrown", &fault, 3);
        assert_eq!(log.since(203)["entries"][0]["cursor"], 204);
    }

    #[test]
    fn addresses_lose_their_secrets() {
        assert_eq!(plain("https://me:pw@a.b:8/c?d=e#f"), "https://a.b:8/c");
        assert_eq!(plain("https://a.b"), "https://a.b");
        assert_eq!(plain("data:text/plain,hi"), "data:text/plain,hi");
    }
}
