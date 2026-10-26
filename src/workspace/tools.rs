//! Warp's tools panel: a column between the tab sidebar and the panes, holding the project
//! explorer (and global search, 3a-3).
use super::{WorkspaceView, icon_button, menu::MenuAction};
use crate::{
    explorer::ExplorerEvent,
    search::SearchEvent,
    theme::{Theme, hsla},
};
use gpui::{
    Context, CursorStyle, MouseButton, MouseMoveEvent, Pixels, Point, Window, div, prelude::*, px,
};
use std::path::{Path, PathBuf};

pub(super) const TOOLS_DEFAULT_WIDTH: f32 = 260.0;
const TOOLS_MIN_WIDTH: f32 = 180.0;
const TOOLS_MAX_SHARE: f32 = 0.4;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tool {
    Explorer,
    Search,
}

impl WorkspaceView {
    pub(super) fn toggle_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tools_open = !self.tools_open;
        self.explorer
            .update(cx, |explorer, cx| explorer.set_active(self.tools_open, cx));
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
        self.tool = tool;
        if self.tools_open {
            self.focus_tool(window, cx);
            cx.notify();
        } else {
            self.toggle_tools(window, cx);
        }
    }

    fn focus_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tool {
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
            // ponytail: the line is dropped until bareframe has its own editor (3b).
            SearchEvent::Open(path, _line) => open_externally(path),
            SearchEvent::Dismiss => self.sync(window, cx),
        }
    }

    /// Warp's roots: the working directories of the active tab's terminals, the focused
    /// one first, with any directory inside another root folded into it.
    pub(super) fn refresh_explorer_roots(&mut self, cx: &mut Context<Self>) {
        if !self.tools_open {
            return;
        }
        let tab = self.model.active();
        let mut panes = tab.layout.panes();
        panes.sort_by_key(|pane| *pane != tab.active_pane);
        let mut roots: Vec<PathBuf> = Vec::new();
        for pane in panes {
            let Some(cwd) = self
                .terminal(pane)
                .and_then(|pane| pane.read(cx).cwd().canonicalize().ok())
                .filter(|cwd| cwd.is_dir())
            else {
                continue;
            };
            if !roots.contains(&cwd) {
                roots.push(cwd);
            }
        }
        let nested: Vec<PathBuf> = roots
            .iter()
            .filter(|root| {
                roots
                    .iter()
                    .any(|other| other != *root && root.starts_with(other))
            })
            .cloned()
            .collect();
        roots.retain(|root| !nested.contains(root));
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
            ExplorerEvent::Open(path) => open_externally(path),
            ExplorerEvent::ContextMenu {
                path,
                root,
                dir,
                position,
            } => self.open_explorer_menu(path, root, *dir, *position, window, cx),
        }
    }

    fn open_explorer_menu(
        &mut self,
        path: &Path,
        root: &Path,
        dir: bool,
        position: Point<Pixels>,
        _: &mut Window,
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
    /// first, as Warp's explorer does.
    pub(super) fn cd_to(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let pane = self.model.active_pane();
        let Some(terminal) = self.terminal(pane) else {
            return;
        };
        let dir = dir.to_string_lossy();
        // Quotes don't stop a typed newline from submitting the line early.
        if dir.chars().any(char::is_control) {
            eprintln!("bareframe: not typing a path with control characters: {dir:?}");
            return;
        }
        let line = format!("\u{15}cd {}\r", shell_quote(&dir));
        terminal.update(cx, |terminal, cx| terminal.type_text(&line, cx));
    }

    pub(super) fn drag_tools_edge(&mut self, event: &MouseMoveEvent, window: &Window) -> bool {
        if !self.resizing_tools {
            return false;
        }
        if !event.dragging() {
            self.resizing_tools = false;
            return true;
        }
        let max = (f32::from(window.viewport_size().width) * TOOLS_MAX_SHARE).max(TOOLS_MIN_WIDTH);
        self.tools_width =
            (f32::from(event.position.x) - self.tools_left).clamp(TOOLS_MIN_WIDTH, max);
        true
    }

    pub(super) fn tools_shown_width(&self, window: &Window) -> f32 {
        self.tools_width
            .min(f32::from(window.viewport_size().width) * TOOLS_MAX_SHARE)
            .max(TOOLS_MIN_WIDTH)
    }

    pub(super) fn render_tools(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
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
                    .child(
                        tool_tab(
                            theme,
                            "tool-explorer",
                            "icons/file.svg",
                            self.tool == Tool::Explorer,
                        )
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_tool(Tool::Explorer, window, cx)
                        })),
                    )
                    .child(
                        tool_tab(
                            theme,
                            "tool-search",
                            "icons/search.svg",
                            self.tool == Tool::Search,
                        )
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_tool(Tool::Search, window, cx)
                        })),
                    )
                    .child(div().flex_1())
                    .child(icon_button("close-tools", "×", theme).on_click(cx.listener(
                        |view, _, window, cx| {
                            cx.stop_propagation();
                            view.toggle_tools(window, cx)
                        },
                    ))),
            )
            .child(div().flex_1().min_h_0().pt_1().map(|this| match self.tool {
                Tool::Explorer => this.child(self.explorer.clone()),
                Tool::Search => this.child(self.global_search.clone()),
            }))
    }

    pub(super) fn render_tools_edge(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        div()
            .flex_none()
            .w(px(5.0))
            .h_full()
            .flex()
            .bg(hsla(theme.terminal_background))
            .cursor(CursorStyle::ResizeLeftRight)
            .child(div().w(px(1.0)).h_full().bg(hsla(theme.border)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    cx.stop_propagation();
                    view.resizing_tools = true;
                }),
            )
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

/// Opens a file with its default macOS application until bareframe has its own viewers.
/// Only plain documents are opened, by their resolved path so a symlink can't disguise a
/// script or app; everything else is revealed in Finder, so a click never runs code.
pub(super) fn open_externally(path: &Path) {
    match std::fs::canonicalize(path) {
        Ok(target) if is_document(&target) => {
            if let Err(error) = std::process::Command::new("/usr/bin/open")
                .arg(&target)
                .spawn()
            {
                eprintln!("bareframe: opening {}: {error}", target.display());
            }
        }
        _ => reveal_in_finder(path),
    }
}

/// A regular, non-executable file with an extension known to open in a viewer or editor.
fn is_document(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    const DOCUMENTS: [&str; 38] = [
        "md", "markdown", "txt", "log", "csv", "tsv", "json", "jsonl", "toml", "yaml", "yml",
        "xml", "ini", "conf", "rs", "c", "h", "cc", "cpp", "hpp", "m", "swift", "zig", "go", "py",
        "js", "ts", "tsx", "css", "png", "jpg", "jpeg", "gif", "svg", "webp", "pdf", "lock",
        "patch",
    ];
    let plain = std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 == 0);
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());
    plain && extension.is_some_and(|extension| DOCUMENTS.contains(&extension.as_str()))
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

/// Single-quotes a path for POSIX shells.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::{is_document, shell_quote};

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
        assert!(is_document(&note));
        assert!(!is_document(&script), "executable bit");
        assert!(!is_document(&command), "not on the list");
        assert!(!is_document(&std::fs::canonicalize(&disguised).unwrap()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn quoting_survives_spaces_and_quotes() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
