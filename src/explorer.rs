//! Project explorer: the active tab's working directories as a lazily loaded file tree,
//! following Warp's tools panel. Gitignored entries stay visible but dimmed.
use crate::theme::{self, Theme, hsla};
use futures::StreamExt;
use gpui::{
    Context, EventEmitter, FocusHandle, MouseButton, MouseDownEvent, Pixels, Point, Task,
    UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

actions!(
    explorer,
    [SelectPrevious, SelectNext, Collapse, Expand, Activate]
);

const KEY_CONTEXT: &str = "Explorer";
const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let context = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("left", Collapse, context),
        KeyBinding::new("right", Expand, context),
        KeyBinding::new("enter", Activate, context),
    ]
}

pub enum ExplorerEvent {
    /// A file was activated; directories expand in place instead.
    Open(PathBuf),
    ContextMenu {
        path: PathBuf,
        root: PathBuf,
        dir: bool,
        position: Point<Pixels>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Child {
    name: String,
    path: PathBuf,
    dir: bool,
    ignored: bool,
}

/// One visible line of the tree.
#[derive(Clone)]
struct Row {
    path: PathBuf,
    root: PathBuf,
    name: String,
    depth: usize,
    dir: bool,
    ignored: bool,
    expanded: bool,
}

pub struct Explorer {
    roots: Vec<PathBuf>,
    expanded: HashSet<PathBuf>,
    /// Directory listings read so far; a watcher event drops the stale ones.
    listings: HashMap<PathBuf, Vec<Child>>,
    /// Directories the rows needed that aren't read yet; render starts reading them.
    wanted: HashSet<PathBuf>,
    /// Reads in flight off the UI thread (a network mount can take seconds), each with a
    /// token so a read made stale by a change on disk is dropped.
    loading: HashMap<PathBuf, u64>,
    next_load: u64,
    selected: Option<PathBuf>,
    show_hidden: bool,
    theme: &'static Theme,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// Present only while the panel is visible, as in Warp.
    watcher: Option<(RecommendedWatcher, Task<()>)>,
}

impl EventEmitter<ExplorerEvent> for Explorer {}

impl Explorer {
    pub fn new(theme: &'static Theme, cx: &mut Context<Self>) -> Self {
        Self {
            roots: Vec::new(),
            expanded: HashSet::new(),
            listings: HashMap::new(),
            wanted: HashSet::new(),
            loading: HashMap::new(),
            next_load: 0,
            selected: None,
            show_hidden: true,
            theme,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            watcher: None,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn set_show_hidden(&mut self, show: bool, cx: &mut Context<Self>) {
        if show != self.show_hidden {
            self.show_hidden = show;
            self.forget_listings();
            cx.notify();
        }
    }

    /// Roots come from the active tab's terminals; a new root starts expanded.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        if roots == self.roots {
            return;
        }
        for root in &roots {
            if !self.roots.contains(root) {
                self.expanded.insert(root.clone());
            }
        }
        self.roots = roots;
        if self.watcher.is_some() {
            self.watch(cx);
        }
        cx.notify();
    }

    /// Watches the roots while the panel is shown; hidden, it drops the watcher and the
    /// cached listings so reopening shows the current disk state.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if active && self.watcher.is_some() {
            return;
        }
        if active {
            self.forget_listings();
            self.watch(cx);
        } else {
            self.watcher = None;
        }
        cx.notify();
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        let (sender, mut receiver) = futures::channel::mpsc::unbounded::<PathBuf>();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            for path in event.map(|event| event.paths).unwrap_or_default() {
                let _ = sender.unbounded_send(path);
            }
        });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                eprintln!("bareframe: watching files: {error}");
                return;
            }
        };
        for root in &self.roots {
            if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
                eprintln!("bareframe: watching {}: {error}", root.display());
            }
        }
        let task = cx.spawn(async move |this, cx| {
            while let Some(first) = receiver.next().await {
                // Editors and builds touch many files at once; refresh once per burst.
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let mut changed = vec![first];
                while let Ok(path) = receiver.try_recv() {
                    changed.push(path);
                }
                if this
                    .update(cx, |this, cx| this.invalidate(&changed, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.watcher = Some((watcher, task));
    }

    fn invalidate(&mut self, changed: &[PathBuf], cx: &mut Context<Self>) {
        // A read still in flight is redone too, so it can't bring back what just changed.
        let before = self.listings.len() + self.loading.len();
        for path in changed {
            self.listings.remove(path);
            self.loading.remove(path);
            if let Some(parent) = path.parent() {
                self.listings.remove(parent);
                self.loading.remove(parent);
            }
        }
        if self.listings.len() + self.loading.len() != before {
            cx.notify();
        }
    }

    fn forget_listings(&mut self) {
        self.listings.clear();
        self.loading.clear();
    }

    /// The directory's entries, or none yet while it is being read.
    fn listing(&mut self, dir: &Path) -> Vec<Child> {
        match self.listings.get(dir) {
            Some(children) => children.clone(),
            None => {
                self.wanted.insert(dir.to_path_buf());
                Vec::new()
            }
        }
    }

    fn load_wanted(&mut self, cx: &mut Context<Self>) {
        for dir in std::mem::take(&mut self.wanted) {
            if self.loading.contains_key(&dir) {
                continue;
            }
            self.next_load += 1;
            let token = self.next_load;
            self.loading.insert(dir.clone(), token);
            let show_hidden = self.show_hidden;
            cx.spawn(async move |this, cx| {
                let read = dir.clone();
                let children = cx
                    .background_executor()
                    .spawn(async move { list_dir(&read, show_hidden) })
                    .await;
                this.update(cx, |this, cx| {
                    if this.loading.get(&dir) == Some(&token) {
                        this.loading.remove(&dir);
                        this.listings.insert(dir, children);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    fn rows(&mut self) -> Vec<Row> {
        let mut rows = Vec::new();
        for root in self.roots.clone() {
            let name = file_name(&root);
            let expanded = self.expanded.contains(&root);
            rows.push(Row {
                path: root.clone(),
                root: root.clone(),
                name,
                depth: 0,
                dir: true,
                ignored: false,
                expanded,
            });
            if expanded {
                self.push_children(&root, &root, 1, &mut rows);
            }
        }
        rows
    }

    fn push_children(&mut self, root: &Path, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        for child in self.listing(dir) {
            let expanded = child.dir && self.expanded.contains(&child.path);
            rows.push(Row {
                path: child.path.clone(),
                root: root.to_path_buf(),
                name: child.name,
                depth,
                dir: child.dir,
                ignored: child.ignored,
                expanded,
            });
            if expanded {
                self.push_children(root, &child.path, depth + 1, rows);
            }
        }
    }

    fn activate(&mut self, row: &Row, cx: &mut Context<Self>) {
        self.selected = Some(row.path.clone());
        if row.dir {
            if !self.expanded.remove(&row.path) {
                self.expanded.insert(row.path.clone());
            }
        } else {
            cx.emit(ExplorerEvent::Open(row.path.clone()));
        }
        cx.notify();
    }

    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|path| rows.iter().position(|row| &row.path == path));
        let next = match current {
            Some(index) => index.saturating_add_signed(step).min(rows.len() - 1),
            None => 0,
        };
        self.selected = Some(rows[next].path.clone());
        self.scroll
            .scroll_to_item(next, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    fn selected_row(&mut self) -> Option<Row> {
        let selected = self.selected.clone()?;
        self.rows().into_iter().find(|row| row.path == selected)
    }

    fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.dir && row.expanded != expanded {
            self.activate(&row, cx);
        } else if !expanded && row.depth > 0 {
            // Left on a file or a closed folder moves up to its parent, as in most trees.
            self.selected = row.path.parent().map(Path::to_path_buf);
            cx.notify();
        }
    }

    fn render_row(&self, row: Row, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = self.theme;
        let selected = self.selected.as_ref() == Some(&row.path);
        let icon_path = if row.dir {
            "icons/folder.svg"
        } else {
            "icons/file.svg"
        };
        let color = hsla(if row.ignored {
            theme.text_muted
        } else {
            theme.text
        });
        let clicked = row.clone();
        let menu = row.clone();
        div()
            .id(gpui::ElementId::Name(
                row.path.to_string_lossy().into_owned().into(),
            ))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(8.0 + INDENT * row.depth as f32))
            .pr_2()
            .rounded_sm()
            .text_color(color)
            .when(row.ignored, |this| this.italic())
            .when(selected, |this| this.bg(hsla(theme.tab_active)))
            .when(!selected, |this| {
                this.hover(|style| style.bg(hsla(theme.tab_hover)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.focus.focus(window, cx);
                this.activate(&clicked, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.selected = Some(menu.path.clone());
                    cx.emit(ExplorerEvent::ContextMenu {
                        path: menu.path.clone(),
                        root: menu.root.clone(),
                        dir: menu.dir,
                        position: event.position,
                    });
                    cx.notify();
                }),
            )
            .child(
                div()
                    .w(px(12.0))
                    .flex_none()
                    .when(row.dir, |this| this.child(chevron(theme, row.expanded))),
            )
            .child(icon(theme, icon_path))
            .child(div().truncate().child(row.name))
            .into_any_element()
    }
}

impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let count = self.rows().len();
        self.load_wanted(cx);
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .font_family(theme::UI_FONT)
            .text_size(px(13.0))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &Collapse, _, cx| this.set_expanded(false, cx)))
            .on_action(cx.listener(|this, _: &Expand, _, cx| this.set_expanded(true, cx)))
            .on_action(cx.listener(|this, _: &Activate, _, cx| {
                if let Some(row) = this.selected_row() {
                    this.activate(&row, cx);
                }
            }))
            .when(count == 0, |this| {
                this.child(
                    div()
                        .p_4()
                        .text_color(hsla(theme.text_muted))
                        .child("No working directory"),
                )
            })
            .child(
                uniform_list(
                    "explorer-rows",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let rows = this.rows();
                        rows[range.start.min(rows.len())..range.end.min(rows.len())]
                            .iter()
                            .cloned()
                            .map(|row| this.render_row(row, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full()
                .px_1(),
            )
    }
}

/// The last part of `path` for display, or the whole path when it has none (`/`).
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into(),
    )
}

/// A tree row's fold mark: down when `open`.
pub fn chevron(theme: &Theme, open: bool) -> impl IntoElement {
    gpui::svg()
        .path(if open {
            "icons/chevron-down.svg"
        } else {
            "icons/chevron-right.svg"
        })
        .flex_none()
        .size(px(12.0))
        .text_color(hsla(theme.text_muted))
}

pub fn icon(theme: &Theme, path: &'static str) -> impl IntoElement {
    gpui::svg()
        .path(path)
        .flex_none()
        .size(px(14.0))
        .text_color(hsla(theme.text_muted))
}

/// One directory level, directories first, then names case-insensitively. Entries the
/// repository's ignore rules would skip are kept and marked, as Warp shows them dimmed.
fn list_dir(dir: &Path, show_hidden: bool) -> Vec<Child> {
    let visible: HashSet<PathBuf> = ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.depth() == 1)
        .map(ignore::DirEntry::into_path)
        .collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut children: Vec<Child> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            Some(Child {
                dir: path.is_dir(),
                ignored: name == ".git" || !visible.contains(&path),
                name,
                path,
            })
        })
        .collect();
    children.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    children
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_marks_ignored_entries_and_sorts_folders_first() {
        let dir = std::env::temp_dir().join(format!("bareframe-explorer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for path in [".git", "src", "target", "Docs"] {
            std::fs::create_dir_all(dir.join(path)).unwrap();
        }
        for (path, text) in [
            (".gitignore", "target/\n*.log\n"),
            ("b.rs", ""),
            ("A.md", ""),
            ("debug.log", ""),
            (".env", ""),
        ] {
            std::fs::write(dir.join(path), text).unwrap();
        }
        let listed = |show_hidden| -> Vec<(String, bool, bool)> {
            list_dir(&dir, show_hidden)
                .into_iter()
                .map(|child| (child.name, child.dir, child.ignored))
                .collect()
        };
        let row = |name: &str, dir, ignored| (name.to_owned(), dir, ignored);
        assert_eq!(
            listed(true),
            [
                row(".git", true, true),
                row("Docs", true, false),
                row("src", true, false),
                row("target", true, true),
                row(".env", false, false),
                row(".gitignore", false, false),
                row("A.md", false, false),
                row("b.rs", false, false),
                row("debug.log", false, true),
            ]
        );
        assert!(
            listed(false)
                .iter()
                .all(|(name, _, _)| !name.starts_with('.'))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
