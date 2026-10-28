//! What an agent does to a page, over the page's own DevTools protocol (see `engine`): read
//! it as an outline, and click, type and wait the way a user would, through Chromium's
//! input events rather than scripts calling `.click()`.
use super::engine;
use futures::future::Either;
use gpui::AsyncApp;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

/// How long a page has to answer one DevTools call.
const DEADLINE: Duration = Duration::from_secs(10);
/// How long `navigate` and `wait_for` wait unless told otherwise.
pub const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);
/// The most lines a snapshot gives; a page's whole tree can run to tens of thousands.
const SNAPSHOT_LINES: usize = 4000;

/// A stable code an agent can act on, and what went wrong.
pub type Failure = (&'static str, String);

/// A DevTools call on `browser`, within `DEADLINE`.
pub async fn call(
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

/// Input for the page, as `call`. Once it is on its way the page may act on it even when no
/// answer comes, so a failure then is `OUTCOME_UNKNOWN`: look before clicking or submitting
/// again.
async fn input(
    browser: i32,
    method: &str,
    params: Value,
    cx: &mut AsyncApp,
) -> Result<Value, Failure> {
    call(browser, method, params, cx)
        .await
        .map_err(|(code, error)| match code {
            "TAB_CLOSED" => (code, error),
            _ => (
                "OUTCOME_UNKNOWN",
                format!("{error}; the page may have acted on it: look before trying again"),
            ),
        })
}

/// The value a page expression evaluates to.
async fn evaluate(browser: i32, expression: &str, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let result = call(
        browser,
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true }),
        cx,
    )
    .await?;
    if let Some(error) = result.get("exceptionDetails") {
        return Err((
            "SCRIPT_ERROR",
            error["text"].as_str().unwrap_or("").to_owned(),
        ));
    }
    Ok(result["result"]["value"].clone())
}

pub async fn snapshot(browser: i32, cx: &mut AsyncApp) -> Result<(String, bool), Failure> {
    let tree = call(browser, "Accessibility.getFullAXTree", json!({}), cx).await?;
    Ok(outline(&tree["nodes"]))
}

pub async fn screenshot(browser: i32, cx: &mut AsyncApp) -> Result<Value, Failure> {
    let shot = call(
        browser,
        "Page.captureScreenshot",
        json!({ "format": "png" }),
        cx,
    )
    .await?;
    Ok(shot["data"].clone())
}

/// Loads `url` and waits for the page to finish loading, up to `WAIT`.
pub async fn navigate(browser: i32, url: &str, cx: &mut AsyncApp) -> Result<(), Failure> {
    let result = call(browser, "Page.navigate", json!({ "url": url }), cx).await?;
    if let Some(error) = result["errorText"]
        .as_str()
        .filter(|error| !error.is_empty())
    {
        return Err(("NAVIGATION_FAILED", format!("{url}: {error}")));
    }
    // The old document can still report `complete` for a moment.
    cx.background_executor().timer(POLL).await;
    loaded(browser, cx).await
}

/// Waits for the page to finish loading, up to `WAIT`.
pub async fn loaded(browser: i32, cx: &mut AsyncApp) -> Result<(), Failure> {
    if wait(browser, "document.readyState === 'complete'", WAIT, cx).await? {
        Ok(())
    } else {
        Err((
            "TIMEOUT",
            format!("the page did not finish loading in {WAIT:?}"),
        ))
    }
}

/// Polls `expression` until it is true or `limit` passes; whether it came true.
async fn wait(
    browser: i32,
    expression: &str,
    limit: Duration,
    cx: &mut AsyncApp,
) -> Result<bool, Failure> {
    let start = std::time::Instant::now();
    loop {
        if evaluate(browser, expression, cx).await? == Value::Bool(true) {
            return Ok(true);
        }
        if start.elapsed() >= limit {
            return Ok(false);
        }
        cx.background_executor().timer(POLL).await;
    }
}

/// Waits for `text` to be shown in the page, up to `limit`.
pub async fn wait_for_text(
    browser: i32,
    text: &str,
    limit: Duration,
    cx: &mut AsyncApp,
) -> Result<(), Failure> {
    let expression = format!(
        "!!document.body && document.body.innerText.includes({})",
        Value::from(text)
    );
    if wait(browser, &expression, limit, cx).await? {
        Ok(())
    } else {
        Err(("TIMEOUT", format!("{:?} did not appear in {limit:?}", text)))
    }
}

/// A script object for the element a snapshot's `ref` names.
async fn object(browser: i32, node: i64, cx: &mut AsyncApp) -> Result<String, Failure> {
    let resolved = call(
        browser,
        "DOM.resolveNode",
        json!({ "backendNodeId": node }),
        cx,
    )
    .await
    .map_err(|_| {
        (
            "REF_NOT_FOUND",
            format!("no element ref={node}; take a new snapshot"),
        )
    })?;
    resolved["object"]["objectId"]
        .as_str()
        .map(str::to_owned)
        .ok_or(("REF_NOT_FOUND", format!("no element ref={node}")))
}

/// Runs `function` with `this` as `object` and returns its value.
async fn on_object(
    browser: i32,
    object: &str,
    function: &str,
    arguments: Value,
    cx: &mut AsyncApp,
) -> Result<Value, Failure> {
    let result = call(
        browser,
        "Runtime.callFunctionOn",
        json!({
            "objectId": object,
            "functionDeclaration": function,
            "arguments": arguments,
            "returnByValue": true,
        }),
        cx,
    )
    .await?;
    Ok(result["result"]["value"].clone())
}

/// Clicks the element `node` (a snapshot's ref) as a mouse would: scrolled into view, at
/// its middle, after checking nothing covers that point.
pub async fn click(browser: i32, node: i64, cx: &mut AsyncApp) -> Result<(), Failure> {
    let element = object(browser, node, cx).await?;
    let _ = call(
        browser,
        "DOM.scrollIntoViewIfNeeded",
        json!({ "backendNodeId": node }),
        cx,
    )
    .await;
    let model = call(
        browser,
        "DOM.getBoxModel",
        json!({ "backendNodeId": node }),
        cx,
    )
    .await
    .map_err(|_| ("NOT_VISIBLE", format!("ref={node} has no box on the page")))?;
    let quad: Vec<f64> = model["model"]["content"]
        .as_array()
        .map(|quad| quad.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    if quad.len() != 8 {
        return Err(("NOT_VISIBLE", format!("ref={node} has no box on the page")));
    }
    let x = (quad[0] + quad[2] + quad[4] + quad[6]) / 4.0;
    let y = (quad[1] + quad[3] + quad[5] + quad[7]) / 4.0;
    // What a click there would reach must be the element or inside it.
    let hit = call(
        browser,
        "DOM.getNodeForLocation",
        json!({ "x": x as i64, "y": y as i64, "includeUserAgentShadowDOM": false }),
        cx,
    )
    .await?;
    let hit = hit["backendNodeId"].as_i64().unwrap_or_default();
    if hit != node {
        let hit = object(browser, hit, cx).await?;
        let inside = on_object(
            browser,
            &element,
            "function(other) { return this === other || this.contains(other); }",
            json!([{ "objectId": hit }]),
            cx,
        )
        .await?;
        if inside != Value::Bool(true) {
            return Err((
                "OBSCURED",
                format!("something covers ref={node} at its middle"),
            ));
        }
    }
    for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
        input(
            browser,
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }),
            cx,
        )
        .await?;
    }
    Ok(())
}

/// Replaces the text of the field `node` with `text`, typed in as input: the page sees the
/// same events as from a keyboard.
pub async fn fill(browser: i32, node: i64, text: &str, cx: &mut AsyncApp) -> Result<(), Failure> {
    let element = object(browser, node, cx).await?;
    let editable = on_object(
        browser,
        &element,
        "function() {
            const field = this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement;
            if (!field && !this.isContentEditable) return false;
            this.focus();
            if (field) { this.select(); } else {
                const range = document.createRange();
                range.selectNodeContents(this);
                getSelection().removeAllRanges();
                getSelection().addRange(range);
            }
            return true;
        }",
        json!([]),
        cx,
    )
    .await?;
    if editable != Value::Bool(true) {
        return Err(("NOT_EDITABLE", format!("ref={node} is not a text field")));
    }
    if text.is_empty() {
        press(browser, "Backspace", cx).await
    } else {
        input(browser, "Input.insertText", json!({ "text": text }), cx).await?;
        Ok(())
    }
}

/// Presses and releases `key`: a name (`Enter`, `Tab`, `Escape`, `Backspace`, `Delete`,
/// arrows, `Home`, `End`, `PageUp`, `PageDown`) or one character.
pub async fn press(browser: i32, key: &str, cx: &mut AsyncApp) -> Result<(), Failure> {
    let (code, virtual_key, text) = key_of(key).ok_or((
        "UNSUPPORTED_KEY",
        format!("{key:?} is not a key name or one character"),
    ))?;
    let mut down = json!({
        "type": "keyDown",
        "key": key,
        "code": code,
        "windowsVirtualKeyCode": virtual_key,
    });
    if !text.is_empty() {
        down["text"] = Value::from(text);
    }
    input(browser, "Input.dispatchKeyEvent", down, cx).await?;
    let up = json!({
        "type": "keyUp",
        "key": key,
        "code": code,
        "windowsVirtualKeyCode": virtual_key,
    });
    input(browser, "Input.dispatchKeyEvent", up, cx).await?;
    Ok(())
}

/// The DOM `code`, Windows virtual key and typed text for a key.
fn key_of(key: &str) -> Option<(String, i64, String)> {
    let named = match key {
        "Enter" => (13, "\r"),
        "Tab" => (9, "\t"),
        "Escape" => (27, ""),
        "Backspace" => (8, ""),
        "Delete" => (46, ""),
        "ArrowLeft" => (37, ""),
        "ArrowUp" => (38, ""),
        "ArrowRight" => (39, ""),
        "ArrowDown" => (40, ""),
        "Home" => (36, ""),
        "End" => (35, ""),
        "PageUp" => (33, ""),
        "PageDown" => (34, ""),
        _ => {
            let mut chars = key.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let upper = ch.to_ascii_uppercase();
            let (code, virtual_key) = match upper {
                'A'..='Z' => (format!("Key{upper}"), upper as i64),
                '0'..='9' => (format!("Digit{upper}"), upper as i64),
                ' ' => ("Space".to_owned(), 32),
                _ => (String::new(), 0),
            };
            return Some((code, virtual_key, key.to_owned()));
        }
    };
    Some((key.to_owned(), named.0, named.1.to_owned()))
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
    use super::{key_of, outline};
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

    #[test]
    fn keys_are_names_or_single_characters() {
        assert_eq!(key_of("Enter"), Some(("Enter".into(), 13, "\r".into())));
        assert_eq!(key_of("a"), Some(("KeyA".into(), 65, "a".into())));
        assert_eq!(key_of("7"), Some(("Digit7".into(), 55, "7".into())));
        assert_eq!(key_of("한"), Some((String::new(), 0, "한".into())));
        assert_eq!(key_of("Enterr"), None);
    }
}
