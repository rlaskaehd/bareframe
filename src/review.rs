//! Warp's Code review panel: the working tree's changes as file cards with unified diffs,
//! compared with HEAD or with the point a branch split off, plus reverting a file or a
//! hunk and committing. Git runs off the UI thread (see `repo`).
use crate::{
    explorer,
    repo::{self, Base, FileReview},
    text_field::{TextField, TextFieldEvent},
    theme::{self, Theme, hsla},
};
use bareframe_core::git::{Change, Entry, LineKind, Status};
use gpui::{
    AnyElement, ClipboardItem, Context, Entity, EventEmitter, FontWeight, ListAlignment,
    ListOffset, ListState, Subscription, Task, Window, div, list, prelude::*, px,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub enum ReviewEvent {
    /// Open the file, as the explorer would.
    Open(PathBuf),
    /// The working tree or the repository changed through the panel.
    Changed,
    Close,
}

#[derive(Clone, Copy)]
enum Row {
    File(usize),
    Hunk(usize, usize),
    Line(usize, usize, usize),
    Note(usize),
}

/// Something that throws work away, waiting for the user to confirm it.
enum Pending {
    File(usize),
    Hunk(usize, usize),
}

struct CommitForm {
    input: Entity<TextField>,
    /// Files left out of the commit; every change is in by default.
    excluded: HashSet<String>,
    _events: [Subscription; 2],
}

pub struct CodeReview {
    root: Option<PathBuf>,
    base: Base,
    status: Status,
    files: Vec<FileReview>,
    branches: Vec<String>,
    collapsed: HashSet<String>,
    rows: Vec<Row>,
    list: ListState,
    loading: bool,
    error: Option<String>,
    /// Why the last revert or commit failed; kept until the next one, through reloads.
    failure: Option<String>,
    /// A revert or commit is running. Reloads wait for it, so a refresh set off by its own
    /// writes to the index can't drop it and lose its outcome.
    changing: bool,
    _change: Option<Task<()>>,
    choosing_base: bool,
    /// Why committing is blocked (a merge in progress, conflicts), read with the changes.
    blocker: Option<&'static str>,
    pending: Option<Pending>,
    commit: Option<CommitForm>,
    /// Bumped by each reload so an older, slower one can't overwrite a newer result.
    generation: u64,
    _load: Option<Task<()>>,
    theme: &'static Theme,
    mono: String,
}

impl EventEmitter<ReviewEvent> for CodeReview {}

impl CodeReview {
    pub fn new(theme: &'static Theme, mono: String) -> Self {
        Self {
            root: None,
            base: Base::Uncommitted,
            status: Status::default(),
            files: Vec::new(),
            branches: Vec::new(),
            collapsed: HashSet::new(),
            rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(400.0)),
            loading: false,
            error: None,
            failure: None,
            changing: false,
            _change: None,
            choosing_base: false,
            blocker: None,
            pending: None,
            commit: None,
            generation: 0,
            _load: None,
            theme,
            mono,
        }
    }

    pub fn set_theme(&mut self, theme: &'static Theme, mono: String, cx: &mut Context<Self>) {
        self.theme = theme;
        self.mono = mono;
        cx.notify();
    }

    /// The repository to show; a new one starts again from "Uncommitted changes".
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if root == self.root {
            return;
        }
        self.root = root;
        self.base = Base::Uncommitted;
        self.collapsed.clear();
        self.pending = None;
        self.commit = None;
        self.reload(cx);
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let Some(root) = self.root.clone() else {
            self.files.clear();
            self.status = Status::default();
            self.error = None;
            self.rebuild_rows();
            return cx.notify();
        };
        if self.changing {
            // The change reloads when it is done.
            return;
        }
        let base = self.base.clone();
        self.loading = true;
        cx.notify();
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let review = repo::review(&root, &base);
                    let blocker = review
                        .as_ref()
                        .ok()
                        .and_then(|review| repo::commit_blocker(&root, &review.status));
                    (review, repo::branches(&root), blocker)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let (review, branches, blocker) = loaded;
                this.loading = false;
                this.branches = branches;
                this.blocker = blocker;
                match review {
                    Ok(review) => {
                        this.status = review.status;
                        this.files = review.files;
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                this.pending = None;
                this.rebuild_rows();
                cx.notify();
            })
            .ok();
        }));
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for (index, file) in self.files.iter().enumerate() {
            rows.push(Row::File(index));
            if self.collapsed.contains(&file.path) {
                continue;
            }
            match &file.diff {
                Some(diff) => {
                    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
                        rows.push(Row::Hunk(index, hunk_index));
                        rows.extend(
                            (0..hunk.lines.len()).map(|line| Row::Line(index, hunk_index, line)),
                        );
                    }
                }
                None => rows.push(Row::Note(index)),
            }
        }
        // Keep the reader's place across a refresh.
        let top = self.list.logical_scroll_top();
        self.list.reset(rows.len());
        if top.item_ix < rows.len() {
            self.list.scroll_to(ListOffset {
                item_ix: top.item_ix,
                offset_in_item: top.offset_in_item,
            });
        }
        self.rows = rows;
    }

    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(file) = self.files.get(index)
            && !self.collapsed.remove(&file.path)
        {
            self.collapsed.insert(file.path.clone());
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn set_base(&mut self, base: Base, cx: &mut Context<Self>) {
        self.choosing_base = false;
        if base != self.base {
            self.base = base;
            self.pending = None;
            self.reload(cx);
        }
        cx.notify();
    }

    fn uncommitted(&self) -> bool {
        self.base == Base::Uncommitted
    }

    /// Runs a change to the working tree off the UI thread, then reloads.
    fn change(
        &mut self,
        work: impl FnOnce(&Path) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.changing {
            return;
        }
        self.pending = None;
        self.failure = None;
        self.loading = true;
        self.changing = true;
        cx.notify();
        self._change = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { work(&root) })
                .await;
            this.update(cx, |this, cx| {
                this.changing = false;
                this.loading = false;
                this.failure = result.err();
                cx.emit(ReviewEvent::Changed);
                this.reload(cx);
            })
            .ok();
        }));
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        match self.pending.take() {
            Some(Pending::File(index)) => {
                let Some(file) = self.files.get(index) else {
                    return;
                };
                let entry = Entry {
                    path: file.path.clone(),
                    change: file.change.clone(),
                };
                self.change(move |root| repo::revert_file(root, &entry), cx);
            }
            Some(Pending::Hunk(index, hunk)) => {
                let Some(patch) = self
                    .files
                    .get(index)
                    .and_then(|file| file.diff.as_ref())
                    .and_then(|diff| diff.hunk_patch(hunk))
                else {
                    return;
                };
                self.change(move |root| repo::revert_hunk(root, &patch), cx);
            }
            None => {}
        }
    }

    fn open_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit.is_some() {
            return;
        }
        let input = cx.new(|cx| TextField::search(self.theme.accent, window, cx));
        let events = cx.subscribe(&input, |this, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Confirm(_) => this.commit(cx),
            TextFieldEvent::Cancel => {
                this.commit = None;
                cx.notify();
            }
        });
        let changes = cx.observe(&input, |_, _, cx| cx.notify());
        self.pending = None;
        self.commit = Some(CommitForm {
            input,
            excluded: HashSet::new(),
            _events: [events, changes],
        });
        cx.notify();
    }

    /// The files the commit will take: every change unless some were unticked.
    fn commit_paths(&self) -> Vec<String> {
        let Some(form) = &self.commit else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        for file in &self.files {
            if form.excluded.contains(&file.path) {
                continue;
            }
            // A rename commits both its sides.
            if let Change::Renamed { from } = &file.change {
                paths.push(from.clone());
            }
            paths.push(file.path.clone());
        }
        paths
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.commit else {
            return;
        };
        let message = form.input.read(cx).text().trim().to_owned();
        let paths = self.commit_paths();
        // Enter in the message field must respect a merge in progress as the button does.
        if message.is_empty() || paths.is_empty() || self.loading || self.blocker.is_some() {
            return;
        }
        // Everything ticked: let git take every change, deletions and renames included.
        let paths = if form.excluded.is_empty() {
            Vec::new()
        } else {
            paths
        };
        self.commit = None;
        self.change(
            move |root| repo::commit(root, &message, &paths).map(drop),
            cx,
        );
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let name = self
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let branch = self
            .status
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into());
        let (added, removed) = self.files.iter().fold((0, 0), |(a, r), file| {
            (a + file.counts.0, r + file.counts.1)
        });
        let base = match &self.base {
            Base::Uncommitted => "Uncommitted changes".to_owned(),
            Base::Branch(branch) => format!("Changes since {branch}"),
        };
        let can_commit = self.uncommitted() && !self.files.is_empty() && self.commit.is_none();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(hsla(theme.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{name}: {branch}")),
                    )
                    .when(can_commit, |this| {
                        this.child(button(theme, "review-commit", "Commit", true).on_click(
                            cx.listener(|this, _, window, cx| this.open_commit(window, cx)),
                        ))
                    })
                    .child(
                        theme::icon_button(theme, "review-close", "×")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ReviewEvent::Close))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.0))
                    .child(
                        div()
                            .id("review-base")
                            .px_2()
                            .py(px(2.0))
                            .rounded_md()
                            .border_1()
                            .border_color(hsla(theme.border))
                            .hover(|style| style.bg(hsla(theme.tab_hover)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choosing_base = !this.choosing_base;
                                cx.notify();
                            }))
                            .child(format!("{base} ▾")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(hsla(theme.text_muted))
                            .child(format!("{} files", self.files.len())),
                    )
                    .child(counts(theme, added, removed)),
            )
            .when(self.choosing_base, |this| this.child(self.render_bases(cx)))
            .children(self.render_commit_form(cx))
            .children(self.render_pending(cx))
            .when_some(
                self.failure.clone().or(self.error.clone()),
                |this, error| {
                    this.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(hsla(theme.group_colors[3]))
                            .child(error),
                    )
                },
            )
            .into_any_element()
    }

    fn render_bases(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let current = self.status.branch.clone();
        let choices = std::iter::once((Base::Uncommitted, "Uncommitted changes".to_owned())).chain(
            self.branches
                .iter()
                .filter(|branch| Some(*branch) != current.as_ref())
                .map(|branch| (Base::Branch(branch.clone()), branch.clone())),
        );
        div()
            .flex()
            .flex_col()
            .rounded_md()
            .border_1()
            .border_color(hsla(theme.border))
            .bg(hsla(theme.tab_active))
            .py_1()
            .text_size(px(12.0))
            .children(choices.enumerate().map(|(index, (base, label))| {
                let chosen = base == self.base;
                div()
                    .id(("review-base-choice", index))
                    .px_2()
                    .py(px(3.0))
                    .when(chosen, |this| this.text_color(hsla(theme.accent)))
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_base(base.clone(), cx)))
                    .child(label)
            }))
            .into_any_element()
    }

    fn render_commit_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.commit.as_ref()?;
        let theme = self.theme;
        let blocker = self.blocker;
        let count = self
            .files
            .iter()
            .filter(|file| !form.excluded.contains(&file.path))
            .count();
        let empty = form.input.read(cx).text().trim().is_empty();
        let ready = blocker.is_none() && count > 0 && !empty;
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(hsla(theme.border))
                .bg(hsla(theme.tab_active))
                .text_size(px(12.0))
                .child(
                    div()
                        .id("review-message")
                        .relative()
                        .h(px(28.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(hsla(theme.border))
                        .bg(hsla(theme.terminal_background))
                        .overflow_hidden()
                        .cursor_text()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(form) = &this.commit {
                                form.input.update(cx, |input, cx| input.focus(window, cx));
                            }
                        }))
                        .when(empty, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .left(px(9.0))
                                    .text_color(hsla(theme.text_muted))
                                    .child("Commit message"),
                            )
                        })
                        .child(form.input.clone()),
                )
                .child(
                    div()
                        .text_color(hsla(theme.text_muted))
                        .child(match blocker {
                            Some(reason) => reason.to_owned(),
                            None => format!(
                                "Commits {count} of {} changed files. Untick files below to leave them out.",
                                self.files.len()
                            ),
                        }),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(button(theme, "review-commit-cancel", "Cancel", false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.commit = None;
                                cx.notify();
                            }),
                        ))
                        .child(
                            button(theme, "review-commit-go", "Commit", ready)
                                .when(!ready, |this| this.opacity(0.5))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if ready {
                                        this.commit(cx)
                                    }
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_pending(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = self.theme;
        let question = match self.pending.as_ref()? {
            Pending::File(index) => {
                let file = self.files.get(*index)?;
                match file.change {
                    Change::Untracked | Change::Added => {
                        format!("Move the new file {} to the Trash?", file.path)
                    }
                    _ => format!("Discard all changes to {}?", file.path),
                }
            }
            Pending::Hunk(index, _) => {
                format!("Discard this change in {}?", self.files.get(*index)?.path)
            }
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(hsla(theme.group_colors[3]))
                .text_size(px(12.0))
                .child(question)
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button(theme, "review-keep", "Cancel", false).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.pending = None;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            button(theme, "review-discard", "Discard", true)
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_row(&self, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        match row {
            Row::File(index) => self.render_file(index, cx),
            Row::Hunk(index, hunk) => self.render_hunk(index, hunk, cx),
            Row::Line(index, hunk, line) => self.render_line(index, hunk, line),
            Row::Note(index) => div()
                .w_full()
                .px_2()
                .py_1()
                .text_size(px(12.0))
                .text_color(hsla(theme.text_muted))
                .child(self.files[index].note.unwrap_or(""))
                .into_any_element(),
        }
    }

    fn render_hunk(&self, index: usize, hunk: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let header = self.files[index]
            .diff
            .as_ref()
            .map(|diff| diff.hunks[hunk].header.clone())
            .unwrap_or_default();
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .mt_1()
            .px_2()
            .h(px(22.0))
            .bg(hsla(theme.tab_active))
            .text_size(px(11.0))
            .text_color(hsla(theme.text_muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(self.mono.clone())
                    .child(header),
            )
            .when(self.uncommitted(), |this| {
                this.child(
                    small_text_button(
                        theme,
                        ("review-hunk-revert", index * 10_000 + hunk),
                        "Revert",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.commit = None;
                        this.pending = Some(Pending::Hunk(index, hunk));
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }

    fn render_line(&self, index: usize, hunk: usize, line: usize) -> AnyElement {
        let theme = self.theme;
        let Some(line) = self.files[index]
            .diff
            .as_ref()
            .map(|diff| &diff.hunks[hunk].lines[line])
        else {
            return div().into_any_element();
        };
        let (sign, tint) = match line.kind {
            LineKind::Added => ("+", Some(theme.palette[2])),
            LineKind::Removed => ("-", Some(theme.palette[1])),
            LineKind::Context => (" ", None),
            LineKind::NoNewline => ("", None),
        };
        let number = |number: Option<u32>| {
            div()
                .flex_none()
                .w(px(36.0))
                .pr_1()
                .flex()
                .justify_end()
                .text_color(hsla(theme.text_muted))
                .children(number.map(|number| number.to_string()))
        };
        div()
            .flex()
            .w_full()
            .min_h(px(18.0))
            .font_family(self.mono.clone())
            .text_size(px(12.0))
            .when_some(tint, |this, tint| this.bg(hsla(tint).opacity(0.14)))
            .child(number(line.old))
            .child(number(line.new))
            .child(
                div()
                    .flex_none()
                    .w(px(14.0))
                    .text_color(tint.map_or(hsla(theme.text_muted), hsla))
                    .child(sign),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(line.kind == LineKind::NoNewline, |this| {
                        this.text_color(hsla(theme.text_muted))
                    })
                    .child(line.text.trim_end_matches('\r').to_owned()),
            )
            .into_any_element()
    }
    fn render_file(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let file = &self.files[index];
        let collapsed = self.collapsed.contains(&file.path);
        let (letter, color) = match file.change {
            Change::Modified | Change::TypeChanged => ("M", theme.palette[3]),
            Change::Added => ("A", theme.palette[2]),
            Change::Untracked => ("U", theme.palette[2]),
            Change::Deleted => ("D", theme.palette[1]),
            Change::Renamed { .. } => ("R", theme.palette[4]),
            Change::Conflicted => ("C", theme.palette[1]),
        };
        let (dir, name) = match file.path.rsplit_once('/') {
            Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
            None => (String::new(), file.path.clone()),
        };
        let path = self.root.as_ref().map(|root| root.join(&file.path));
        let copy_path = file.path.clone();
        let committing = self.commit.as_ref();
        let excluded = committing.is_some_and(|form| form.excluded.contains(&file.path));
        let toggled = file.path.clone();
        div()
            .id(("review-file", index))
            .flex()
            .items_center()
            .gap_2()
            .mt_2()
            .w_full()
            .px_2()
            .h(px(30.0))
            .rounded_md()
            .border_1()
            .border_color(hsla(theme.border))
            .bg(hsla(theme.tab_active))
            .text_size(px(12.0))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(index, cx)))
            .when(committing.is_some(), |this| {
                this.child(
                    div()
                        .id(("review-include", index))
                        .text_color(hsla(theme.text_muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(form) = &mut this.commit
                                && !form.excluded.remove(&toggled)
                            {
                                form.excluded.insert(toggled.clone());
                            }
                            cx.notify();
                        }))
                        .child(if excluded { "☐" } else { "☑" }),
                )
            })
            .child(explorer::chevron(theme, !collapsed))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::BOLD)
                    .text_color(hsla(color))
                    .child(letter),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap_1()
                    .overflow_hidden()
                    .child(div().flex_none().child(name))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(hsla(theme.text_muted))
                            .child(dir),
                    ),
            )
            .child(counts(theme, file.counts.0, file.counts.1))
            .child(
                small_text_button(theme, ("review-copy", index), "Copy").on_click(cx.listener(
                    move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()));
                    },
                )),
            )
            .when(
                self.uncommitted() && file.change != Change::Conflicted,
                |this| {
                    this.child(
                        small_text_button(theme, ("review-revert", index), "Revert").on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.commit = None;
                                this.pending = Some(Pending::File(index));
                                cx.notify();
                            }),
                        ),
                    )
                },
            )
            .when(file.change != Change::Deleted, |this| {
                this.child(
                    small_text_button(theme, ("review-open", index), "Open").on_click(cx.listener(
                        move |_, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(path) = &path {
                                cx.emit(ReviewEvent::Open(path.clone()));
                            }
                        },
                    )),
                )
            })
            .into_any_element()
    }
}

impl Render for CodeReview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let empty = self.files.is_empty();
        let message = match (&self.root, self.loading) {
            (None, _) => Some("Not in a git repository"),
            (Some(_), true) if empty => Some("Loading…"),
            (Some(_), false) if empty && self.error.is_none() => Some("No changes"),
            _ => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.chrome))
            .font_family(theme::UI_FONT)
            .text_size(px(13.0))
            .text_color(hsla(theme.text))
            .child(self.render_header(cx))
            .when_some(message, |this, message| {
                this.child(
                    div()
                        .p_4()
                        .text_color(hsla(theme.text_muted))
                        .child(message),
                )
            })
            .child(
                list(
                    self.list.clone(),
                    // Each row fills the panel's width, inset like Warp's cards.
                    cx.processor(|this, index: usize, _, cx| {
                        let row = match this.rows.get(index) {
                            Some(row) => this.render_row(*row, cx),
                            None => div().into_any_element(),
                        };
                        div().w_full().px_2().child(row).into_any_element()
                    }),
                )
                .flex_1()
                .pb_2(),
            )
    }
}

/// `+N −M` in the diff colors.
pub fn counts(theme: &Theme, added: u64, removed: u64) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap_1()
        .text_size(px(12.0))
        .child(
            div()
                .text_color(hsla(theme.palette[2]))
                .child(format!("+{added}")),
        )
        .child(
            div()
                .text_color(hsla(theme.palette[1]))
                .child(format!("−{removed}")),
        )
}

fn button(
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    theme::button(theme, id, label, primary)
        .px_2()
        .py(px(3.0))
        .text_size(px(12.0))
}

fn small_text_button(
    theme: &Theme,
    id: impl Into<gpui::ElementId>,
    label: &'static str,
) -> gpui::Stateful<gpui::Div> {
    theme::ghost_button(theme, id, label)
        .flex_none()
        .px_1()
        .rounded_sm()
        .text_size(px(11.0))
}
