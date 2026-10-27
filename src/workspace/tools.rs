//! Warp's tools panel: a column between the tab sidebar and the panes, holding the project
//! explorer (and global search, 3a-3).
use super::{WorkspaceView, icon_button, menu::MenuAction};
use crate::{
    explorer::ExplorerEvent,
    search::SearchEvent,
    theme::{Theme, hsla},
};
use bareframe_core::config::quote;
use gpui::{Context, Pixels, Point, Window, div, prelude::*, px};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tool {
    Explorer,
    Search,
}

impl WorkspaceView {
    /// Settings › Tools panel can switch each tool off.
    fn tool_enabled(&self, tool: Tool) -> bool {
        self.settings.flag(match tool {
            Tool::Explorer => "project-explorer",
            Tool::Search => "global-search",
        })
    }

    /// The tool on show: the chosen one if enabled, else any enabled one.
    pub(super) fn shown_tool(&self) -> Option<Tool> {
        [self.tool, Tool::Explorer, Tool::Search]
            .into_iter()
            .find(|tool| self.tool_enabled(*tool))
    }

    /// Closes the panel once Settings has switched every tool off, and lets the explorer
    /// watch its folders only while it is on screen.
    pub(super) fn update_tools(&mut self, cx: &mut Context<Self>) {
        if self.shown_tool().is_none() {
            self.tools_open = false;
        }
        let active = self.tools_open && self.shown_tool() == Some(Tool::Explorer);
        self.explorer
            .update(cx, |explorer, cx| explorer.set_active(active, cx));
    }

    pub(super) fn toggle_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tools_open && self.shown_tool().is_none() {
            return;
        }
        self.tools_open = !self.tools_open;
        self.update_tools(cx);
        if self.tools_open {
            self.refresh_explorer_roots(cx);
            self.focus_tool(window, cx);
        } else {
            self.sync(window, cx);
        }
        cx.notify();
    }

    /// Warp's rule for a tool's shortcut: if that tool is already showing, close the
    /// panel; otherwise open the panel on it and focus it.
    pub(super) fn show_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tools_open && self.tool == tool {
            return self.toggle_tools(window, cx);
        }
        self.open_tool(tool, window, cx);
    }

    pub(super) fn open_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tool_enabled(tool) {
            return;
        }
        self.tool = tool;
        if self.tools_open {
            self.update_tools(cx);
            self.focus_tool(window, cx);
            cx.notify();
        } else {
            self.toggle_tools(window, cx);
        }
    }

    fn focus_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.shown_tool().unwrap_or(self.tool) {
            Tool::Explorer => self
                .explorer
                .update(cx, |explorer, cx| explorer.focus(window, cx)),
            Tool::Search => self
                .global_search
                .update(cx, |search, cx| search.focus(window, cx)),
        }
    }

    pub(super) fn on_search_event(
        &mut self,
        event: &SearchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEvent::Open(path, line) => self.open_file_at(path, Some(*line), window, cx),
            SearchEvent::Dismiss => self.sync(window, cx),
        }
    }

    /// The root follows the focused pane's working directory, as Warp's explorer follows
    /// the focused session; a file pane falls back to the first terminal in the tab.
    pub(super) fn refresh_explorer_roots(&mut self, cx: &mut Context<Self>) {
        if !self.tools_open {
            return;
        }
        let tab = self.model.active();
        let mut panes = tab.layout.panes();
        panes.sort_by_key(|pane| *pane != tab.active_pane);
        let roots: Vec<PathBuf> = panes
            .into_iter()
            .find_map(|pane| {
                self.terminal(pane)
                    .and_then(|pane| pane.read(cx).cwd().canonicalize().ok())
                    .filter(|cwd| cwd.is_dir())
            })
            .into_iter()
            .collect();
        self.global_search
            .update(cx, |search, cx| search.set_roots(roots.clone(), cx));
        self.explorer
            .update(cx, |explorer, cx| explorer.set_roots(roots, cx));
    }

    pub(super) fn on_explorer_event(
        &mut self,
        event: &ExplorerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExplorerEvent::Open(path) => self.open_file(path, window, cx),
            ExplorerEvent::ContextMenu {
                path,
                root,
                dir,
                position,
            } => self.open_explorer_menu(path, root, *dir, *position, cx),
        }
    }

    fn open_explorer_menu(
        &mut self,
        path: &Path,
        root: &Path,
        dir: bool,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let relative = path.strip_prefix(root).map_or_else(
            |_| path.display().to_string(),
            |relative| relative.display().to_string(),
        );
        let mut items = Vec::new();
        if dir {
            items.push((
                "cd to directory".into(),
                MenuAction::CdTo(path.to_path_buf()),
            ));
            items.push((
                "Open in new tab".into(),
                MenuAction::NewTabAt(path.to_path_buf()),
            ));
        } else {
            items.push(("Open".into(), MenuAction::OpenPath(path.to_path_buf())));
        }
        items.push((String::new(), MenuAction::Separator));
        items.push((
            "Reveal in Finder".into(),
            MenuAction::Reveal(path.to_path_buf()),
        ));
        items.push((
            "Copy path".into(),
            MenuAction::CopyText(path.display().to_string()),
        ));
        if !relative.is_empty() {
            items.push(("Copy relative path".into(), MenuAction::CopyText(relative)));
        }
        self.open_menu(position, items, cx);
    }

    /// Types `cd` into the focused terminal of the active tab, clearing any half-typed line
    /// first, as Warp's explorer does. A terminal running a program (an editor, a pager, an
    /// ssh session) would take the line as keys, so the directory opens in a new tab instead.
    pub(super) fn cd_to(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.model.active_pane();
        let Some(terminal) = self
            .terminal(pane)
            .filter(|terminal| terminal.read(cx).at_prompt())
        else {
            return self.new_tab_at(dir.to_path_buf(), window, cx);
        };
        let dir = dir.to_string_lossy();
        // Quotes don't stop a typed newline from submitting the line early.
        if dir.chars().any(char::is_control) {
            eprintln!("bareframe: not typing a path with control characters: {dir:?}");
            return self.sync(window, cx);
        }
        let line = format!("\u{15}cd {}\r", quote(&dir));
        terminal.update(cx, |terminal, cx| terminal.type_text(&line, cx));
        self.sync(window, cx);
    }

    pub(super) fn render_tools(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let shown = self.shown_tool().unwrap_or(self.tool);
        // With one tool left in Settings there is nothing to switch between.
        let switch = self.tool_enabled(Tool::Explorer) && self.tool_enabled(Tool::Search);
        div()
            .w(px(width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.chrome))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .h(px(40.0))
                    .px_2()
                    .border_b_1()
                    .border_color(hsla(theme.border))
                    .when(switch, |this| {
                        this.child(
                            tool_tab(
                                theme,
                                "tool-explorer",
                                "icons/file.svg",
                                shown == Tool::Explorer,
                            )
                            .on_click(cx.listener(
                                |view, _, window, cx| view.open_tool(Tool::Explorer, window, cx),
                            )),
                        )
                        .child(
                            tool_tab(
                                theme,
                                "tool-search",
                                "icons/search.svg",
                                shown == Tool::Search,
                            )
                            .on_click(cx.listener(
                                |view, _, window, cx| view.open_tool(Tool::Search, window, cx),
                            )),
                        )
                    })
                    .child(div().flex_1())
                    .child(icon_button("close-tools", "×", theme).on_click(cx.listener(
                        |view, _, window, cx| {
                            cx.stop_propagation();
                            view.toggle_tools(window, cx)
                        },
                    ))),
            )
            .child(div().flex_1().min_h_0().pt_1().map(|this| match shown {
                Tool::Explorer => this.child(self.explorer.clone()),
                Tool::Search => this.child(self.global_search.clone()),
            }))
    }
}

fn tool_tab(
    theme: &Theme,
    id: &'static str,
    icon: &'static str,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded_md()
        .when(selected, |this| this.bg(hsla(theme.tab_active)))
        .when(!selected, |this| {
            this.hover(|style| style.bg(hsla(theme.tab_hover)))
        })
        .child(
            gpui::svg()
                .path(icon)
                .size(px(15.0))
                .text_color(hsla(if selected {
                    theme.text
                } else {
                    theme.text_muted
                })),
        )
}

/// Opens a file outside bareframe until it has its own viewers. Only plain documents are
/// opened, by their resolved path so a symlink can't disguise a script or app: text and
/// source in the default text editor (a `.py` default handler may run it), images and
/// PDFs in their viewer. Everything else is revealed in Finder, so a click never runs code.
pub(super) fn open_externally(path: &Path) {
    let target = std::fs::canonicalize(path).ok();
    let mut open = std::process::Command::new("/usr/bin/open");
    match target.as_deref().map(document_kind) {
        Some(Some(Document::Text)) => open.arg("-t"),
        Some(Some(Document::Media)) => &mut open,
        _ => return reveal_in_finder(path),
    };
    let target = target.expect("a document kind implies a resolved path");
    if let Err(error) = open.arg(&target).spawn() {
        eprintln!("bareframe: opening {}: {error}", target.display());
    }
}

#[derive(Debug, PartialEq)]
enum Document {
    Text,
    Media,
}

/// A regular, non-executable file with a known document extension.
fn document_kind(path: &Path) -> Option<Document> {
    use std::os::unix::fs::PermissionsExt;
    const TEXT: [&str; 31] = [
        "md", "markdown", "txt", "log", "csv", "tsv", "json", "jsonl", "toml", "yaml", "yml",
        "xml", "ini", "conf", "rs", "c", "h", "cc", "cpp", "hpp", "m", "swift", "zig", "go", "py",
        "js", "ts", "tsx", "css", "lock", "patch",
    ];
    const MEDIA: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "pdf"];
    let plain = std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 == 0);
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    if !plain {
        None
    } else if TEXT.contains(&extension.as_str()) {
        Some(Document::Text)
    } else if MEDIA.contains(&extension.as_str()) {
        Some(Document::Media)
    } else {
        None
    }
}

pub(super) fn reveal_in_finder(path: &Path) {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        eprintln!("bareframe: revealing {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::{Document, document_kind};

    #[test]
    fn only_plain_documents_are_opened() {
        let dir = std::env::temp_dir().join(format!("bareframe-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("notes.md");
        std::fs::write(&note, "").unwrap();
        let script = dir.join("run.py");
        std::fs::write(&script, "").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let command = dir.join("deploy.command");
        std::fs::write(&command, "").unwrap();
        // A link named like a document resolves to what it points at.
        let disguised = dir.join("readme.md");
        let _ = std::fs::remove_file(&disguised);
        std::os::unix::fs::symlink(&command, &disguised).unwrap();
        assert_eq!(document_kind(&note), Some(Document::Text));
        assert_eq!(document_kind(&script), None, "executable bit");
        assert_eq!(document_kind(&command), None, "not on the list");
        assert_eq!(
            document_kind(&std::fs::canonicalize(&disguised).unwrap()),
            None
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
