//! Changing files from the explorer, as in Warp and VS Code: new files and folders and
//! renames typed into the tree, deleting to the Trash, and moving by dragging onto a folder.
//! Panes already showing a file that moved keep their old path; it is opened again from its
//! new place.
use super::{Explorer, ExplorerEvent, file_name};
use crate::{
    file_icons::{self, FileIcon},
    text_field::{TextField, TextFieldEvent},
    theme::{Theme, hsla},
    widgets,
};
use gpui::{Context, Entity, Subscription, Window, div, prelude::*, px};
use std::path::{Path, PathBuf};

pub(super) enum Edit {
    /// A new file or folder in `dir`, named in a row at the top of its children.
    New {
        dir: PathBuf,
        folder: bool,
    },
    Rename(PathBuf),
}

pub(super) struct Editing {
    pub(super) edit: Edit,
    pub(super) input: Entity<TextField>,
    _events: Subscription,
}

/// A file or folder lifted in the tree; it lands in the folder it is dropped on, and a file
/// bareframe opens itself can be dropped beside a pane to open there.
#[derive(Clone)]
pub struct DraggedEntry {
    pub path: PathBuf,
    theme: &'static Theme,
}

impl Render for DraggedEntry {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let icon = if self.path.is_dir() {
            FileIcon::FOLDER
        } else {
            file_icons::for_path(&self.path)
        };
        widgets::lifted(theme)
            .flex()
            .items_center()
            .gap(px(6.0))
            .px_2()
            .py_1()
            .text_size(px(13.0))
            .child(widgets::file_icon(theme, icon))
            .child(file_name(&self.path))
    }
}

impl Explorer {
    /// Starts naming a new file or folder in `dir` (a file's folder, when a file was
    /// right-clicked).
    pub fn new_entry(
        &mut self,
        dir: PathBuf,
        folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expanded.insert(dir.clone());
        self.start_edit(Edit::New { dir, folder }, "", window, cx);
    }

    pub fn rename_entry(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = file_name(&path);
        self.start_edit(Edit::Rename(path), &name, window, cx);
    }

    /// Moves `path` to the Trash, where it can be put back from.
    pub fn delete_entry(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.error = trash::delete(path)
            .err()
            .map(|error| format!("Could not move {} to the Trash: {error}", file_name(path)));
        self.changed(path, cx);
    }

    fn start_edit(&mut self, edit: Edit, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let accent = self.theme.accent;
        let input = cx.new(|cx| TextField::new(text, accent, window, cx));
        let events = cx.subscribe(&input, |explorer, _, event: &TextFieldEvent, cx| {
            explorer.finish_edit(event, cx)
        });
        self.error = None;
        self.editing = Some(Editing {
            edit,
            input,
            _events: events,
        });
        cx.notify();
    }

    fn finish_edit(&mut self, event: &TextFieldEvent, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        cx.notify();
        let TextFieldEvent::Confirm(name) = event else {
            return;
        };
        let name = name.trim();
        // Nothing typed, or the name kept, is a change of mind.
        let unchanged = match &editing.edit {
            Edit::Rename(path) => file_name(path) == name,
            Edit::New { .. } => false,
        };
        if name.is_empty() || unchanged {
            return;
        }
        let done = match &editing.edit {
            Edit::New { dir, folder } => create(dir, name, *folder),
            Edit::Rename(path) => rename(path, name),
        };
        match done {
            Ok(path) => {
                self.selected = Some(path.clone());
                self.changed(&path, cx);
                if matches!(editing.edit, Edit::New { folder: false, .. }) {
                    cx.emit(ExplorerEvent::Open(path));
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn drop_entry(&mut self, from: &Path, into: &Path, cx: &mut Context<Self>) {
        // Let go where it was picked up: nothing to do.
        if from == into {
            return;
        }
        match move_into(from, into) {
            Ok(path) => {
                self.expanded.insert(into.to_path_buf());
                self.selected = Some(path.clone());
                self.changed(from, cx);
                self.changed(&path, cx);
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    /// Reads `path`'s folder again now rather than when the watcher reports it.
    fn changed(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.invalidate(&[path.to_path_buf()], cx);
        cx.notify();
    }

    /// The row where a new entry is named, or a renamed one shows its field.
    pub(super) fn render_input(&self, depth: usize, folder: bool) -> Option<gpui::AnyElement> {
        let editing = self.editing.as_ref()?;
        let theme = self.theme;
        let icon = if folder {
            FileIcon::FOLDER
        } else {
            FileIcon::FILE
        };
        Some(
            div()
                .h(px(super::ROW_HEIGHT))
                .w_full()
                .flex()
                .items_center()
                .gap(px(6.0))
                .pl(px(8.0 + super::INDENT * depth as f32 + 12.0))
                .pr_2()
                .child(widgets::file_icon(theme, icon))
                .child(name_field(theme, editing.input.clone()))
                .into_any_element(),
        )
    }

    pub(super) fn dragged(&self, path: &Path) -> DraggedEntry {
        DraggedEntry {
            path: path.to_path_buf(),
            theme: self.theme,
        }
    }
}

/// The box around the field a name is typed in.
pub(super) fn name_field(theme: &Theme, input: Entity<TextField>) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(hsla(theme.accent))
        .bg(hsla(theme.terminal_background))
        .child(input)
}

/// A name for something inside a folder: one path component, nothing that would reach out
/// of the folder or break a line.
fn valid_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(format!("`{name}` is not a valid name"));
    }
    if name.chars().any(char::is_control) {
        return Err("A name can't hold control characters".into());
    }
    Ok(name)
}

fn free(path: PathBuf) -> Result<PathBuf, String> {
    match path.symlink_metadata() {
        Ok(_) => Err(format!("{} already exists", file_name(&path))),
        Err(_) => Ok(path),
    }
}

/// Makes an empty file or a folder named `name` in `dir`; never replaces anything.
fn create(dir: &Path, name: &str, folder: bool) -> Result<PathBuf, String> {
    let path = free(dir.join(valid_name(name)?))?;
    let made = if folder {
        std::fs::create_dir(&path)
    } else {
        std::fs::File::create_new(&path).map(drop)
    };
    made.map(|_| path)
        .map_err(|error| format!("Could not create {name}: {error}"))
}

fn rename(path: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = path.parent().ok_or("Can't rename this folder")?;
    let target = dir.join(valid_name(name)?);
    // On a disk that ignores case (macOS's default), `a.txt` → `A.txt` finds the file
    // itself under the new name; that is a rename, not a clash.
    let target = if same_file(path, &target) {
        target
    } else {
        free(target)?
    };
    std::fs::rename(path, &target)
        .map(|_| target)
        .map_err(|error| format!("Could not rename {}: {error}", file_name(path)))
}

fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (a.symlink_metadata(), b.symlink_metadata()) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Moves `path` into the folder `into`, keeping its name.
fn move_into(path: &Path, into: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or("Can't move this folder")?;
    if path.parent() == Some(into) {
        return Ok(path.to_path_buf());
    }
    if into.starts_with(path) {
        return Err(format!("Can't move {} into itself", file_name(path)));
    }
    let target = free(into.join(name))?;
    std::fs::rename(path, &target)
        .map(|_| target)
        .map_err(|error| format!("Could not move {}: {error}", file_name(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_folders_are_made_renamed_and_moved_without_replacing_anything() {
        let dir = std::env::temp_dir().join(format!("bareframe-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = create(&dir, "a.txt", false).unwrap();
        let folder = create(&dir, "sub", true).unwrap();
        assert!(file.is_file() && folder.is_dir());
        assert!(
            create(&dir, "a.txt", false).is_err(),
            "never replaces a file"
        );
        for bad in ["", "..", "x/y", "a\nb"] {
            assert!(create(&dir, bad, false).is_err(), "{bad:?}");
        }
        let renamed = rename(&file, "b.txt").unwrap();
        assert_eq!(renamed, dir.join("b.txt"));
        assert!(!file.exists());
        // Only the case changes, on a disk that may ignore case.
        let upper = rename(&renamed, "B.txt").unwrap();
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(names.contains(&"B.txt".into()), "{names:?}");
        let renamed = rename(&upper, "b.txt").unwrap();
        assert!(rename(&renamed, "sub").is_err(), "the name is taken");
        let moved = move_into(&renamed, &folder).unwrap();
        assert_eq!(moved, folder.join("b.txt"));
        assert!(move_into(&folder, &folder.join("deeper")).is_err());
        assert_eq!(move_into(&moved, &folder).unwrap(), moved, "already there");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
