//! What an agent does to a page, over the page's own DevTools protocol (see `engine`): read
//! it as an outline, and click, type and wait the way a user would, through Chromium's
//! input events rather than scripts calling `.click()`.
use super::engine;
use futures::future::Either;
use gpui::AsyncApp;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, time::Duration};

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

thread_local! {
    /// Each page's world of our own (see `untouched`), while its document lasts.
    static WORLDS: RefCell<HashMap<i32, Value>> = RefCell::default();
}

/// Forgets what was kept for `browser`, once it closed.
pub fn forget(browser: i32) {
    WORLDS.with_borrow_mut(|worlds| worlds.remove(&browser));
}

/// The element `node` as seen from a world of our own beside the page's scripts: what the
/// page did to the DOM's functions and prototypes does not reach code run on it there, so
/// a page can't steer where a click lands or hide what covers it.
async fn untouched(browser: i32, node: i64, cx: &mut AsyncApp) -> Result<String, Failure> {
    for fresh in [false, true] {
        let world = match WORLDS.with_borrow(|worlds| worlds.get(&browser).cloned()) {
            Some(world) if !fresh => world,
            // None yet, or the page moved to another document and took it along.
            _ => {
                let tree = call(browser, "Page.getFrameTree", json!({}), cx).await?;
                let frame = &tree["frameTree"]["frame"]["id"];
                let created = call(
                    browser,
                    "Page.createIsolatedWorld",
                    json!({ "frameId": frame, "worldName": "bareframe" }),
                    cx,
                )
                .await?;
                let world = created["executionContextId"].clone();
                WORLDS.with_borrow_mut(|worlds| worlds.insert(browser, world.clone()));
                world
            }
        };
        let params = json!({ "backendNodeId": node, "executionContextId": world });
        if let Ok(resolved) = call(browser, "DOM.resolveNode", params, cx).await
            && let Some(object) = resolved["object"]["objectId"].as_str()
        {
            return Ok(object.to_owned());
        }
    }
    Err((
        "REF_NOT_FOUND",
        format!("no element ref={node}; take a new snapshot"),
    ))
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

/// Where the element `node` (a snapshot's ref) is for the mouse, in the viewport's CSS
/// pixels that mouse events take: its middle, brought to the middle of the screen first
/// when off it (unless `stay`), whether a press there would land on it rather than on
/// something covering it, and whether the point is on screen at all.
async fn locate(
    browser: i32,
    node: i64,
    stay: bool,
    cx: &mut AsyncApp,
) -> Result<(f64, f64, bool, bool), Failure> {
    let element = untouched(browser, node, cx).await?;
    let found = on_object(
        browser,
        &element,
        "function(stay) {
            const box = () => {
                if (this.nodeType === 1) return this.getBoundingClientRect();
                const range = document.createRange();
                range.selectNode(this);
                return range.getBoundingClientRect();
            };
            const middle = b => [b.left + b.width / 2, b.top + b.height / 2];
            const shown = ([x, y]) => x >= 0 && y >= 0 && x < innerWidth && y < innerHeight;
            let rect = box();
            if (!stay && !shown(middle(rect))) {
                (this.nodeType === 1 ? this : this.parentElement)
                    ?.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
                rect = box();
            }
            if (rect.width === 0 && rect.height === 0) return null;
            const [x, y] = middle(rect);
            // Into open shadow roots, as a press goes.
            let hit = document.elementFromPoint(x, y);
            while (hit && hit.shadowRoot) {
                const inner = hit.shadowRoot.elementFromPoint(x, y);
                if (!inner || inner === hit) break;
                hit = inner;
            }
            const reached = !!hit
                && (hit === this || this.contains(hit) || (this.nodeType !== 1 && hit.contains(this)));
            return { x, y, reached, shown: shown([x, y]) };
        }",
        json!([{ "value": stay }]),
        cx,
    )
    .await?;
    let (Some(x), Some(y)) = (found["x"].as_f64(), found["y"].as_f64()) else {
        return Err(("NOT_VISIBLE", format!("ref={node} has no box on the page")));
    };
    Ok((x, y, found["reached"] == true, found["shown"] == true))
}

/// Where a mouse reaches the element `node`: on screen, at its middle, with nothing
/// covering that point.
async fn reach(browser: i32, node: i64, cx: &mut AsyncApp) -> Result<(f64, f64), Failure> {
    let (x, y, reached, _) = locate(browser, node, false, cx).await?;
    if !reached {
        return Err((
            "OBSCURED",
            format!("something covers ref={node} at its middle"),
        ));
    }
    Ok((x, y))
}

/// Clicks the element `node` as a mouse would, where `reach` finds it: `count` times
/// (2 is a double-click) with `button` (`left`, `right` or `middle`).
pub async fn click(
    browser: i32,
    node: i64,
    button: &str,
    count: u64,
    cx: &mut AsyncApp,
) -> Result<(), Failure> {
    if !matches!(button, "left" | "right" | "middle") {
        return Err((
            "BAD_REQUEST",
            format!("{button:?} is not left, right or middle"),
        ));
    }
    let (x, y) = reach(browser, node, cx).await?;
    if button == "right" {
        super::client::quiet_menu(browser);
    }
    let moved = json!({ "type": "mouseMoved", "x": x, "y": y });
    input(browser, "Input.dispatchMouseEvent", moved, cx).await?;
    // Each press counts up, as a person's quick presses do: the page sees click, then
    // dblclick.
    for clicks in 1..=count.clamp(1, 3) {
        for kind in ["mousePressed", "mouseReleased"] {
            let event = json!({ "type": kind, "x": x, "y": y, "button": button,
                                "clickCount": clicks });
            input(browser, "Input.dispatchMouseEvent", event, cx).await?;
        }
    }
    Ok(())
}

/// Drags the element `from` onto the element `to` with the left button, as a mouse would.
/// A page's own dragging (sliders, sortable lists) follows the moves; a drag the page
/// hands to Chromium (`draggable`) comes back as `intercepted` instead of starting the
/// system's, and is dropped on `to` from here: `true` then.
pub async fn drag(
    browser: i32,
    from: i64,
    to: i64,
    intercepted: futures::channel::oneshot::Receiver<Value>,
    cx: &mut AsyncApp,
) -> Result<bool, Failure> {
    let (x1, y1) = reach(browser, from, cx).await?;
    let (x2, y2, reached, shown) = locate(browser, to, true, cx).await?;
    if !shown {
        return Err((
            "NOT_VISIBLE",
            format!("ref={to} is off the screen while ref={from} is on it; scroll so both show"),
        ));
    }
    if !reached {
        return Err((
            "OBSCURED",
            format!("something covers ref={to} at its middle, where the drop would land"),
        ));
    }
    call(
        browser,
        "Input.setInterceptDrags",
        json!({ "enabled": true }),
        cx,
    )
    .await?;
    // The button is down from the press until the release.
    let mouse = |kind: &str, x: f64, y: f64| {
        let buttons = if kind == "mouseReleased" { 0 } else { 1 };
        json!({ "type": kind, "x": x, "y": y, "button": "left", "buttons": buttons,
                "clickCount": 1 })
    };
    let dragged = async {
        input(
            browser,
            "Input.dispatchMouseEvent",
            mouse("mouseMoved", x1, y1),
            cx,
        )
        .await?;
        input(
            browser,
            "Input.dispatchMouseEvent",
            mouse("mousePressed", x1, y1),
            cx,
        )
        .await?;
        for step in 1..=10 {
            let f = f64::from(step) / 10.0;
            let (x, y) = (x1 + (x2 - x1) * f, y1 + (y2 - y1) * f);
            input(
                browser,
                "Input.dispatchMouseEvent",
                mouse("mouseMoved", x, y),
                cx,
            )
            .await?;
        }
        let wait = cx.background_executor().timer(Duration::from_millis(300));
        let handed = match futures::future::select(intercepted, std::pin::pin!(wait)).await {
            Either::Left((Ok(data), _)) => Some(data),
            _ => None,
        };
        if let Some(data) = &handed {
            for kind in ["dragEnter", "dragOver", "drop"] {
                let event = json!({ "type": kind, "x": x2, "y": y2, "data": data });
                input(browser, "Input.dispatchDragEvent", event, cx).await?;
            }
        }
        input(
            browser,
            "Input.dispatchMouseEvent",
            mouse("mouseReleased", x2, y2),
            cx,
        )
        .await?;
        Ok(handed.is_some())
    }
    .await;
    let _ = call(
        browser,
        "Input.setInterceptDrags",
        json!({ "enabled": false }),
        cx,
    )
    .await;
    dragged
}

/// Moves the mouse onto the element `node` without pressing: menus and tips that open on
/// hover open.
pub async fn hover(browser: i32, node: i64, cx: &mut AsyncApp) -> Result<(), Failure> {
    let (x, y) = reach(browser, node, cx).await?;
    let moved = json!({ "type": "mouseMoved", "x": x, "y": y });
    input(browser, "Input.dispatchMouseEvent", moved, cx).await?;
    Ok(())
}

/// Turns the mouse wheel `pages` screens toward `direction` (`up`, `down`, `left`, `right`)
/// over the element `node`, or the middle of the page: what scrolls is what a user's wheel
/// would scroll there, inner panes and pages that load more as they go included.
pub async fn scroll(
    browser: i32,
    node: Option<i64>,
    direction: &str,
    pages: f64,
    cx: &mut AsyncApp,
) -> Result<(), Failure> {
    let metrics = call(browser, "Page.getLayoutMetrics", json!({}), cx).await?;
    let view = &metrics["cssVisualViewport"];
    let width = view["clientWidth"].as_f64().unwrap_or(800.0);
    let height = view["clientHeight"].as_f64().unwrap_or(600.0);
    let (dx, dy) = match direction {
        "down" => (0.0, height * pages),
        "up" => (0.0, -height * pages),
        "right" => (width * pages, 0.0),
        "left" => (-width * pages, 0.0),
        _ => {
            return Err((
                "BAD_REQUEST",
                format!("{direction:?} is not up, down, left or right"),
            ));
        }
    };
    let (x, y) = match node {
        Some(node) => reach(browser, node, cx).await?,
        None => (width / 2.0, height / 2.0),
    };
    let wheel = json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy });
    input(browser, "Input.dispatchMouseEvent", wheel, cx).await?;
    Ok(())
}

/// Picks the option of the drop-down `node` whose value or label is `choice`, as a user
/// would: the page gets `input` and `change`. The option's label comes back.
pub async fn select(
    browser: i32,
    node: i64,
    choice: &str,
    cx: &mut AsyncApp,
) -> Result<String, Failure> {
    let element = object(browser, node, cx).await?;
    let picked = on_object(
        browser,
        &element,
        "function(choice) {
            if (!(this instanceof HTMLSelectElement)) return { error: 'NOT_SELECT' };
            const options = [...this.options];
            const option = options.find(o => o.value === choice)
                || options.find(o => o.label.trim() === choice.trim());
            if (!option || option.disabled) {
                return { error: 'NO_OPTION', options: options.filter(o => !o.disabled)
                    .slice(0, 50).map(o => o.label.trim()) };
            }
            this.focus();
            option.selected = true;
            this.dispatchEvent(new Event('input', { bubbles: true }));
            this.dispatchEvent(new Event('change', { bubbles: true }));
            return { label: option.label.trim() };
        }",
        json!([{ "value": choice }]),
        cx,
    )
    .await?;
    match picked["error"].as_str() {
        None => Ok(picked["label"].as_str().unwrap_or_default().to_owned()),
        Some("NOT_SELECT") => Err((
            "NOT_SELECT",
            format!("ref={node} is not a drop-down; click a custom one open instead"),
        )),
        Some(_) => Err((
            "NO_OPTION",
            format!("no option {choice:?}; there are {}", picked["options"]),
        )),
    }
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
    for kind in ["keyDown", "keyUp"] {
        let mut event = json!({
            "type": kind,
            "key": key,
            "code": code,
            "windowsVirtualKeyCode": virtual_key,
        });
        // The press types the character.
        if kind == "keyDown" && !text.is_empty() {
            event["text"] = Value::from(text.as_str());
        }
        input(browser, "Input.dispatchKeyEvent", event, cx).await?;
    }
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
