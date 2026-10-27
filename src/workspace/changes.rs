//! The diff button in the top bar and the Code review panel on the window's right: which
//! repository the active pane is in, its change counts, and keeping both current.
use super::{
    WorkspaceView,
    columns::{Column, edge_line},
};
use crate::{
    repo::{self, Summary},
    review::{self, ReviewEvent},
    theme::hsla,
    watch::{Watch, watch},
};
use gpui::{Context, Window, div, prelude::*, px};
use notify::RecursiveMode;
use std::{path::PathBuf, time::Duration};

impl WorkspaceView {
    /// Looks up the active pane's repository and its numbers again. Without `force`, only
    /// when the pane's directory changed since the last look.
    pub(super) fn refresh_repo(&mut self, force: bool, cx: &mut Context<Self>) {
        let dir = self.active_cwd(cx);
        if !force && self.repo_dir.as_ref() == Some(&dir) {
            return;
        }
        self.repo_dir = Some(dir.clone());
        self._repo_task = Some(cx.spawn(async move |view, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let root = repo::root_of(&dir)?;
                    let summary = repo::summary(&root).ok()?;
                    Some((root, summary))
                })
                .await;
            view.update(cx, |view, cx| view.set_repo(found, cx)).ok();
        }));
    }

    fn set_repo(&mut self, found: Option<(PathBuf, Summary)>, cx: &mut Context<Self>) {
        let root = found.as_ref().map(|(root, _)| root.clone());
        let moved = root != self.repo.as_ref().map(|(root, _)| root.clone());
        let changed = found.as_ref().map(|(_, summary)| summary)
            != self.repo.as_ref().map(|(_, summary)| summary);
        self.repo = found;
        if moved {
            self._repo_watch = root.clone().and_then(|root| self.watch_repo(root, cx));
            self.review
                .update(cx, |review, cx| review.set_root(root, cx));
        } else if changed && self.review_open {
            self.review.update(cx, |review, cx| review.reload(cx));
        }
        cx.notify();
    }

    /// Edits saved from anywhere change the numbers, so the working tree is watched; git's
    /// own files only matter where they record a commit, a checkout or staging.
    // ponytail: a build writing thousands of ignored files still wakes this once a
    // second; filter through the repository's ignore rules if that shows up.
    fn watch_repo(&mut self, root: PathBuf, cx: &mut Context<Self>) -> Option<Watch> {
        // A dotfiles repository at the home folder would mean watching everything under it;
        // there the numbers update on commands and focus changes only.
        if crate::files::home_dir().is_some_and(|home| home == root) {
            return None;
        }
        watch(
            &root,
            RecursiveMode::Recursive,
            Duration::from_millis(800),
            |event| {
                event.paths.iter().any(|path| {
                    let inside_git = path.components().any(|part| part.as_os_str() == ".git");
                    !inside_git
                        || path
                            .file_name()
                            .is_some_and(|name| name == "index" || name == "HEAD")
                })
            },
            |view, cx| view.refresh_repo(true, cx),
            cx,
        )
    }

    pub(super) fn on_review_event(
        &mut self,
        event: &ReviewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ReviewEvent::Open(path) => self.open_file(path, window, cx),
            ReviewEvent::Changed => self.refresh_repo(true, cx),
            ReviewEvent::Close => self.toggle_review(window, cx),
        }
    }

    pub(super) fn toggle_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review_open = !self.review_open;
        if self.review_open {
            self.refresh_repo(true, cx);
            self.review.update(cx, |review, cx| review.reload(cx));
        } else {
            self.sync(window, cx);
        }
        cx.notify();
    }

    /// The top bar's diff button, as Warp's: always there (unless Settings hide it), with
    /// `+N −M` once lines changed, and dimmed outside a repository.
    pub(super) fn render_diff_button(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.settings.flag("code-review-button") {
            return None;
        }
        let theme = self.theme();
        let counts = self
            .repo
            .as_ref()
            .map(|(_, summary)| (summary.added, summary.removed))
            .filter(|(added, removed)| *added > 0 || *removed > 0);
        let color = match (&self.repo, self.review_open) {
            (None, _) => theme.text_muted,
            (Some(_), true) => theme.text,
            (Some(_), false) => theme.text_muted,
        };
        Some(
            div()
                .id("diff-button")
                .flex()
                .items_center()
                .gap_1()
                .mr_2()
                .px_2()
                .h(px(24.0))
                .rounded_md()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .when(self.repo.is_none(), |this| this.opacity(0.5))
                .when(self.review_open, |this| this.bg(hsla(theme.tab_active)))
                .hover(|style| style.bg(hsla(theme.tab_hover)))
                .on_click(cx.listener(|view, _, window, cx| {
                    cx.stop_propagation();
                    view.toggle_review(window, cx)
                }))
                .child(
                    gpui::svg()
                        .path("icons/diff.svg")
                        .size(px(15.0))
                        .text_color(hsla(color)),
                )
                .children(counts.map(|(added, removed)| review::counts(theme, added, removed))),
        )
    }

    pub(super) fn render_review(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        div()
            .relative()
            .flex()
            .flex_none()
            .h_full()
            .child(edge_line(theme))
            .child(div().w(px(width)).h_full().child(self.review.clone()))
            // Last, so it lies over the panel's own left edge.
            .child(self.resize_grip(Column::Review, cx).left_0())
    }
}
