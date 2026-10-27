//! The editor's side of language servers, as in Warp's code editor: the text kept in sync,
//! errors and warnings underlined, a card on hover, Cmd+click to a definition, and the
//! footer that offers to install or enable the server for the project.
use super::{EditorEvent, EditorPane};
use crate::{
    lsp::{Encoding, LanguageServers, LspEvent, State, Status, python::Interpreter},
    theme::{self, hsla},
    widgets,
};
use bareframe_core::{
    lsp::{byte_column, path_to_uri, server_for, uri_to_path, utf16_column},
    text::Position,
};
use gpui::{Context, Entity, Pixels, Point, Subscription, Task, deferred, div, prelude::*, px};
use serde_json::{Value, json};
use std::{ops::Range, path::Path, time::Duration};

/// How long the pointer rests before a hover card is asked for, as in Warp.
const HOVER_DELAY: Duration = Duration::from_millis(500);
/// Typing is sent to the server once it pauses this long.
const SYNC_DELAY: Duration = Duration::from_millis(200);

pub(super) struct Language {
    servers: Entity<LanguageServers>,
    /// Errors and warnings in the buffer's own positions.
    pub(super) marks: Vec<Mark>,
    hover: Option<Hover>,
    /// A menu the status line opened, at a point in the window.
    popup: Option<(Point<Pixels>, Popup)>,
    _hover: Option<Task<()>>,
    _sync: Option<Task<()>>,
    _subscriptions: [Subscription; 2],
}

/// What the status line has open above it.
enum Popup {
    /// The lightning icon's menu: install, enable, restart or disable the server.
    Server,
    Pythons(Vec<Interpreter>),
}

pub(super) struct Mark {
    pub(super) range: Range<Position>,
    pub(super) error: bool,
    message: String,
}

struct Hover {
    /// Where the pointer rested, in window coordinates.
    at: Point<Pixels>,
    position: Position,
    /// Asked for but not answered yet.
    text: Option<String>,
}

impl EditorPane {
    /// Opens the file with its language server, if its language has one.
    pub(super) fn attach_language(&mut self, cx: &mut Context<Self>) {
        if server_for(&self.path).is_none() {
            return;
        }
        let servers = crate::lsp::global(cx);
        let (path, text) = (self.path.clone(), self.buffer.text());
        servers.update(cx, |servers, cx| servers.open(&path, &text, cx));
        let updates = cx.subscribe(&servers, |editor, _, _: &LspEvent, cx| {
            editor.refresh_marks(cx)
        });
        let release = cx.on_release({
            let servers = servers.clone();
            move |editor: &mut Self, cx| {
                servers.update(cx, |servers, cx| servers.close(&editor.path, cx));
            }
        });
        self.language_server = Some(Language {
            servers,
            marks: Vec::new(),
            hover: None,
            popup: None,
            _hover: None,
            _sync: None,
            _subscriptions: [updates, release],
        });
    }

    /// The text changed: marks are dropped until the server looks again.
    pub(super) fn sync_language(&mut self, cx: &mut Context<Self>) {
        let Some(language) = self.language_server.as_mut() else {
            return;
        };
        language.marks.clear();
        language.hover = None;
        language._sync = Some(cx.spawn(async move |editor, cx| {
            cx.background_executor().timer(SYNC_DELAY).await;
            editor
                .update(cx, |editor, cx| {
                    let text = editor.buffer.text();
                    if let Some(language) = &editor.language_server {
                        let path = editor.path.clone();
                        language
                            .servers
                            .update(cx, |servers, cx| servers.change(&path, &text, cx));
                    }
                })
                .ok();
        }));
    }

    pub(super) fn language_marks(&self) -> &[Mark] {
        self.language_server
            .as_ref()
            .map_or(&[], |language| &language.marks)
    }

    pub(super) fn language_saved(&self, cx: &mut Context<Self>) {
        if let Some(language) = &self.language_server {
            language.servers.read(cx).saved(&self.path);
        }
    }

    fn refresh_marks(&mut self, cx: &mut Context<Self>) {
        let Some(language) = &self.language_server else {
            return;
        };
        let servers = language.servers.read(cx);
        let encoding = servers.encoding(&self.path).unwrap_or(Encoding::Utf16);
        let marks = servers
            .diagnostics(&self.path)
            .iter()
            .map(|diagnostic| {
                let start = self.server_position(diagnostic.start, encoding);
                let mut end = self.server_position(diagnostic.end, encoding);
                // An empty range (a missing `;` at the end of a line) still gets a mark.
                if end <= start {
                    end = self.buffer.next(start);
                }
                Mark {
                    range: start..end,
                    error: diagnostic.error,
                    message: diagnostic.message.clone(),
                }
            })
            .collect();
        if let Some(language) = self.language_server.as_mut() {
            language.marks = marks;
        }
        cx.notify();
    }

    fn server_position(&self, (line, character): (u32, u32), encoding: Encoding) -> Position {
        let line = (line as usize).min(self.buffer.line_count() - 1);
        let text = self.buffer.line(line);
        let column = match encoding {
            Encoding::Utf16 => byte_column(text, character),
            Encoding::Utf8 => character as usize,
        };
        self.buffer.clamp(Position::new(line, column))
    }

    fn position_for_server(&self, position: Position, encoding: Encoding) -> Value {
        let character = match encoding {
            Encoding::Utf16 => utf16_column(self.buffer.line(position.line), position.column),
            Encoding::Utf8 => position.column as u32,
        };
        json!({"line": position.line, "character": character})
    }

    /// Where the request is about: the file and a place in it.
    fn text_position(&self, position: Position, cx: &Context<Self>) -> Option<Value> {
        let language = self.language_server.as_ref()?;
        let encoding = language.servers.read(cx).encoding(&self.path)?;
        Some(json!({
            "textDocument": {"uri": path_to_uri(&self.path)},
            "position": self.position_for_server(position, encoding),
        }))
    }

    /// The pointer moved over the text: a card for where it rests, after a pause.
    pub(super) fn hover_at(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let position = self.position_for(at);
        let Some(language) = self.language_server.as_mut() else {
            return;
        };
        if language.hover.as_ref().map(|hover| hover.position) == position {
            return;
        }
        let shown = language
            .hover
            .take()
            .is_some_and(|hover| hover.text.is_some());
        language._hover = position.map(|position| {
            cx.spawn(async move |editor, cx| {
                cx.background_executor().timer(HOVER_DELAY).await;
                editor
                    .update(cx, |editor, cx| editor.ask_hover(at, position, cx))
                    .ok();
            })
        });
        if shown {
            cx.notify();
        }
    }

    pub(super) fn hide_hover(&mut self, cx: &mut Context<Self>) {
        if let Some(language) = self.language_server.as_mut() {
            language._hover = None;
            if language.hover.take().is_some() {
                cx.notify();
            }
        }
    }

    fn ask_hover(&mut self, at: Point<Pixels>, position: Position, cx: &mut Context<Self>) {
        let params = self.text_position(position, cx);
        let Some(language) = self.language_server.as_mut() else {
            return;
        };
        let messages: Vec<&str> = language
            .marks
            .iter()
            .filter(|mark| mark.range.start <= position && position < mark.range.end)
            .map(|mark| mark.message.as_str())
            .collect();
        let marks = messages.join("\n\n");
        language.hover = Some(Hover {
            at,
            position,
            text: None,
        });
        let reply = params.and_then(|params| {
            let path = self.path.clone();
            language.servers.update(cx, |servers, _| {
                servers.request(&path, "textDocument/hover", params)
            })
        });
        language._hover = Some(cx.spawn(async move |editor, cx| {
            let contents = match reply {
                Some(reply) => reply.await.ok().and_then(Result::ok).map(hover_text),
                None => None,
            };
            let text = [marks, contents.unwrap_or_default()]
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            editor
                .update(cx, |editor, cx| {
                    if let Some(hover) = editor
                        .language_server
                        .as_mut()
                        .and_then(|language| language.hover.as_mut())
                        .filter(|hover| hover.position == position)
                    {
                        hover.text = Some(text);
                        cx.notify();
                    }
                })
                .ok();
        }));
    }

    /// Cmd+click: the definition here, in this file or another one.
    pub(super) fn go_to_definition(&mut self, position: Position, cx: &mut Context<Self>) {
        let Some(params) = self.text_position(position, cx) else {
            return;
        };
        let Some(language) = self.language_server.as_mut() else {
            return;
        };
        let path = self.path.clone();
        let Some(reply) = language.servers.update(cx, |servers, _| {
            servers.request(&path, "textDocument/definition", params)
        }) else {
            return;
        };
        cx.spawn(async move |editor, cx| {
            let Some(target) = reply.await.ok().and_then(Result::ok).and_then(location) else {
                return;
            };
            editor
                .update(cx, |editor, cx| {
                    let (file, start) = target;
                    if file == editor.path {
                        let encoding = editor
                            .language_server
                            .as_ref()
                            .and_then(|language| language.servers.read(cx).encoding(&file))
                            .unwrap_or(Encoding::Utf16);
                        let position = editor.server_position(start, encoding);
                        editor.select(position, false, cx);
                    } else {
                        cx.emit(EditorEvent::OpenAt(file, u64::from(start.0) + 1));
                    }
                })
                .ok();
        })
        .detach();
    }

    /// The hover card, drawn over everything at the pointer.
    pub(super) fn render_hover(&self) -> Option<impl IntoElement> {
        let hover = self.language_server.as_ref()?.hover.as_ref()?;
        let text = hover.text.clone().filter(|text| !text.is_empty())?;
        let theme = self.theme;
        Some(deferred(
            gpui::anchored()
                .position(hover.at + gpui::point(px(0.0), px(18.0)))
                .snap_to_window()
                .child(
                    div()
                        .id("hover-card")
                        .occlude()
                        .max_w(px(560.0))
                        .max_h(px(320.0))
                        .overflow_y_scroll()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(hsla(theme.border))
                        .bg(hsla(theme.chrome))
                        .shadow_lg()
                        .font_family(self.font.clone())
                        .text_size(px(self.font_size * 0.92))
                        .text_color(hsla(theme.text))
                        .whitespace_normal()
                        .children(
                            text.lines()
                                .map(|line| div().min_h(px(self.font_size)).child(line.to_owned())),
                        ),
                ),
        ))
    }

    /// The language part of the status line: a lightning icon whose dot tells how the
    /// server is doing and whose menu offers it, then errors, warnings and, for Python, the
    /// interpreter, which opens the list to pick another.
    pub(super) fn render_language(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let language = self.language_server.as_ref()?;
        let status = language.servers.read(cx).status(&self.path)?;
        let theme = self.theme;
        let (dot, python) = match status {
            Status::Checking => (None, None),
            Status::Install(_) | Status::Enable(..) => (Some(theme.text_muted), None),
            Status::Running {
                state,
                progress,
                python,
                ..
            } => (Some(state_color(theme, state, progress)), Some(python)),
        };
        let (errors, warnings) =
            language.marks.iter().fold(
                (0, 0),
                |(e, w), mark| {
                    if mark.error { (e + 1, w) } else { (e, w + 1) }
                },
            );
        Some(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .id("lsp-menu")
                        .relative()
                        .flex()
                        .items_center()
                        .p(px(2.0))
                        .rounded_sm()
                        .hover(|style| style.bg(hsla(theme.tab_hover)))
                        .child(widgets::icon(theme, "icons/zap.svg"))
                        .when_some(dot, |this, color| {
                            this.child(
                                div()
                                    .absolute()
                                    .top(px(1.0))
                                    .right(px(1.0))
                                    .size(px(5.0))
                                    .rounded_full()
                                    .bg(hsla(color)),
                            )
                        })
                        .on_click(cx.listener(|editor, event: &gpui::ClickEvent, _, cx| {
                            editor.toggle_popup(event.position(), false, cx)
                        })),
                )
                .when(python.is_some(), |this| {
                    this.child(count(theme, "icons/circle-x.svg", theme.palette[1], errors))
                        .child(count(
                            theme,
                            "icons/triangle-alert.svg",
                            theme.palette[3],
                            warnings,
                        ))
                })
                .when_some(python.flatten(), |this, python| {
                    this.child(
                        div()
                            .id("python-chip")
                            .px(px(5.0))
                            .rounded_sm()
                            .border_1()
                            .border_color(hsla(theme.text_muted).opacity(0.6))
                            .text_color(hsla(theme.text))
                            .hover(|style| style.bg(hsla(theme.tab_hover)))
                            .child(python.chip())
                            .on_click(cx.listener(|editor, event: &gpui::ClickEvent, _, cx| {
                                editor.toggle_popup(event.position(), true, cx)
                            })),
                    )
                }),
        )
    }

    fn toggle_popup(&mut self, at: Point<Pixels>, pythons: bool, cx: &mut Context<Self>) {
        let Some(language) = self.language_server.as_mut() else {
            return;
        };
        language.popup = match language.popup.take() {
            Some(_) => None,
            None if pythons => {
                let found = language.servers.read(cx).interpreters(&self.path).0;
                Some((at, Popup::Pythons(found)))
            }
            None => Some((at, Popup::Server)),
        };
        cx.notify();
    }

    fn close_popup(&mut self, cx: &mut Context<Self>) {
        if let Some(language) = self.language_server.as_mut()
            && language.popup.take().is_some()
        {
            cx.notify();
        }
    }

    /// The open menu, rising from where it was clicked.
    pub(super) fn render_popup(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let language = self.language_server.as_ref()?;
        let (at, popup) = language.popup.as_ref()?;
        let theme = self.theme;
        let note = |text: String| {
            div()
                .px_3()
                .py_1()
                .text_color(hsla(theme.text_muted))
                .child(text)
                .into_any_element()
        };
        let item = |id: (&'static str, usize), label: String| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .flex()
                .flex_col()
                .hover(|style| style.bg(hsla(theme.tab_hover)))
                .child(label)
        };
        let servers = language.servers.read(cx);
        let rows: Vec<gpui::AnyElement> = match popup {
            Popup::Server => match servers.status(&self.path)? {
                Status::Checking => vec![note("Looking for a language server…".into())],
                Status::Install(server) => vec![
                    note("Language support is not enabled for this codebase".into()),
                    item(("lsp", 0), format!("Install {}", server.binary))
                        .on_click(cx.listener(move |editor, _, _, cx| {
                            editor.close_popup(cx);
                            editor.install_language_server(server.install, cx)
                        }))
                        .into_any_element(),
                ],
                Status::Enable(server, name) => vec![
                    note(format!("Language support is not enabled for {name}")),
                    item(("lsp", 0), format!("Enable {}", server.binary))
                        .on_click(cx.listener(|editor, _, _, cx| {
                            editor.close_popup(cx);
                            editor.with_servers(cx, |servers, path, cx| servers.enable(path, cx))
                        }))
                        .into_any_element(),
                ],
                Status::Running {
                    server,
                    state,
                    progress,
                    ..
                } => {
                    let text = match (state, progress) {
                        (State::Failed(error), _) => error.clone(),
                        (_, Some(progress)) => progress.to_owned(),
                        (State::Starting, None) => "starting".into(),
                        (State::Ready, None) => "ready".into(),
                    };
                    vec![
                        note(format!("{}: {text}", server.binary)),
                        item(("lsp", 0), "Restart".into())
                            .on_click(cx.listener(|editor, _, _, cx| {
                                editor.close_popup(cx);
                                editor
                                    .with_servers(cx, |servers, path, cx| servers.restart(path, cx))
                            }))
                            .into_any_element(),
                        item(("lsp", 1), "Disable".into())
                            .on_click(cx.listener(|editor, _, _, cx| {
                                editor.close_popup(cx);
                                editor
                                    .with_servers(cx, |servers, path, cx| servers.disable(path, cx))
                            }))
                            .into_any_element(),
                    ]
                }
            },
            Popup::Pythons(found) => {
                let current = servers.interpreters(&self.path).1;
                let mut rows = Vec::new();
                if found.is_empty() {
                    rows.push(note("No Python interpreters found".into()));
                }
                for (index, interpreter) in found.iter().enumerate() {
                    let python = interpreter.python.clone();
                    rows.push(
                        item(("python", index), interpreter.label.clone())
                            .when(current.as_ref() == Some(interpreter), |this| {
                                this.bg(hsla(theme.tab_active))
                            })
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(hsla(theme.text_muted))
                                    .child(interpreter.python.display().to_string()),
                            )
                            .on_click(cx.listener(move |editor, _, _, cx| {
                                editor.close_popup(cx);
                                let python = python.clone();
                                editor.with_servers(cx, |servers, path, cx| {
                                    servers.choose_python(path, python, cx)
                                })
                            }))
                            .into_any_element(),
                    );
                }
                rows
            }
        };
        Some(deferred(
            gpui::anchored()
                .position(*at)
                .anchor(gpui::Anchor::BottomRight)
                .snap_to_window()
                .child(
                    div()
                        .id("status-popup")
                        .occlude()
                        .min_w(px(220.0))
                        .max_h(px(320.0))
                        .overflow_y_scroll()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(hsla(theme.border))
                        .bg(hsla(theme.chrome))
                        .shadow_lg()
                        .font_family(theme::UI_FONT)
                        .text_size(px(12.0))
                        .text_color(hsla(theme.text))
                        .on_mouse_down_out(cx.listener(|editor, _, _, cx| editor.close_popup(cx)))
                        .children(rows),
                ),
        ))
    }

    fn with_servers(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut LanguageServers, &Path, &mut Context<LanguageServers>),
    ) {
        if let Some(language) = &self.language_server {
            let path = self.path.clone();
            language
                .servers
                .update(cx, |servers, cx| change(servers, &path, cx));
        }
    }

    /// Runs the install command in a terminal, where its output shows. It runs from the
    /// home folder: servers are installed globally, and tools like npm take settings from
    /// the directory they run in. Meanwhile the server is looked for again.
    fn install_language_server(&mut self, command: &str, cx: &mut Context<Self>) {
        let Some(home) = crate::files::home_dir() else {
            return;
        };
        self.with_servers(cx, |servers, path, cx| servers.watch_install(path, cx));
        cx.emit(EditorEvent::RunInTerminal(command.to_owned(), home));
    }
}

/// The server's dot: green when ready, yellow while starting or busy, red when it failed.
fn state_color(theme: &crate::theme::Theme, state: &State, progress: Option<&str>) -> u32 {
    match (state, progress) {
        (State::Failed(_), _) => theme.palette[1],
        (State::Ready, None) => theme.palette[2],
        _ => theme.palette[3],
    }
}

/// An icon and a count, as the status line shows errors and warnings.
fn count(
    theme: &crate::theme::Theme,
    icon: &'static str,
    color: u32,
    count: usize,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            gpui::svg()
                .path(icon)
                .size(px(12.0))
                .text_color(hsla(if count > 0 { color } else { theme.text_muted })),
        )
        .child(count.to_string())
}

/// A hover reply's text: Markdown kept as written, fences dropped.
fn hover_text(reply: Value) -> String {
    fn part(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            Value::Array(parts) => parts.iter().map(part).collect::<Vec<_>>().join("\n\n"),
            Value::Object(_) => value["value"].as_str().unwrap_or_default().to_owned(),
            _ => String::new(),
        }
    }
    part(&reply["contents"])
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// The first place a definition reply points to: a Location, a list of them, or links.
fn location(reply: Value) -> Option<(std::path::PathBuf, (u32, u32))> {
    let first = match reply {
        Value::Array(mut items) if !items.is_empty() => items.swap_remove(0),
        Value::Object(_) => reply,
        _ => return None,
    };
    let uri = first["uri"].as_str().or(first["targetUri"].as_str())?;
    let range = if first.get("targetSelectionRange").is_some() {
        &first["targetSelectionRange"]
    } else {
        &first["range"]
    };
    let start = &range["start"];
    Some((
        uri_to_path(uri)?,
        (
            start["line"].as_u64()? as u32,
            start["character"].as_u64()? as u32,
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_replies_of_every_shape_become_text() {
        let markup =
            json!({"contents": {"kind": "markdown", "value": "```rust\nfn main()\n```\nRuns."}});
        assert_eq!(hover_text(markup), "fn main()\nRuns.");
        let marked = json!({"contents": ["plain", {"language": "py", "value": "x: int"}]});
        assert_eq!(hover_text(marked), "plain\n\nx: int");
        assert_eq!(hover_text(json!(null)), "");
    }

    #[test]
    fn definitions_take_the_first_location_or_link() {
        let range =
            json!({"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 9}});
        let plain = json!({"uri": "file:///a/b.rs", "range": range});
        assert_eq!(location(plain.clone()), Some(("/a/b.rs".into(), (3, 7))));
        assert_eq!(location(json!([plain])), Some(("/a/b.rs".into(), (3, 7))));
        let link = json!([{"targetUri": "file:///c.rs", "targetRange": range,
            "targetSelectionRange": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 3}}}]);
        assert_eq!(location(link), Some(("/c.rs".into(), (1, 2))));
        assert_eq!(location(json!([])), None);
        assert_eq!(location(json!(null)), None);
    }
}
