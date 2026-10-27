//! The status line under the text: the cursor's place and the file's format, and the
//! language server's part with its lightning menu, error and warning counts and Python
//! interpreter chip, and the menus they open.
use super::{EditorEvent, EditorPane};
use crate::{
    lsp::{LanguageServers, State, Status, python::Interpreter},
    theme::{self, Theme, hsla},
    widgets,
};
use bareframe_core::text::LineEnding;
use gpui::{Context, Pixels, Point, deferred, div, prelude::*, px};
use std::path::Path;

/// What the status line has open above it.
pub(super) enum Popup {
    /// The lightning icon's menu: install, enable, restart or disable the server.
    Server,
    Pythons(Vec<Interpreter>),
}

impl EditorPane {
    pub(super) fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let line = self.buffer.line(self.head.line);
        let column = line[..self.head.column.min(line.len())].chars().count() + 1;
        let selected = self.buffer.char_count(self.selection());
        div()
            .flex()
            .flex_none()
            .gap_4()
            .px_3()
            .h(px(22.0))
            .items_center()
            .border_t_1()
            .border_color(hsla(theme.border))
            .font_family(theme::UI_FONT)
            .text_size(px(11.0))
            .text_color(hsla(theme.text_muted))
            .child(format!("Ln {}, Col {column}", self.head.line + 1))
            .when(selected > 0, |this| {
                this.child(format!("{selected} selected"))
            })
            .child(div().flex_1())
            .children(self.render_language(cx))
            .when(self.read_only, |this| this.child("Read-only"))
            .child(match self.buffer.line_ending {
                LineEnding::Lf => "LF",
                LineEnding::CrLf => "CRLF",
            })
            .child("UTF-8")
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
                            .px(px(3.0))
                            .rounded_sm()
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
                let found = language.servers.update(cx, |servers, _| {
                    servers.find_pythons(&self.path);
                    servers.interpreters(&self.path).0
                });
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
fn state_color(theme: &Theme, state: &State, progress: Option<&str>) -> u32 {
    match (state, progress) {
        (State::Failed(_), _) => theme.palette[1],
        (State::Ready, None) => theme.palette[2],
        _ => theme.palette[3],
    }
}

/// An icon and a count, as the status line shows errors and warnings.
fn count(theme: &Theme, icon: &'static str, color: u32, count: usize) -> impl IntoElement {
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
