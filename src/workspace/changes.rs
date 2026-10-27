//! The diff button in the top bar and the Code review panel on the window's right: which
//! repository the active pane is in, its change counts, and keeping both current.
use super::{WorkspaceView, column_width};
use crate::{
    repo::{self, Summary},
    review::{self, ReviewEvent},
    theme::hsla,
    watch::{Watch, watch},
};
use gpui::{Context, CursorStyle, MouseButton, MouseMoveEvent, Window, div, prelude::*, px};
use notify::RecursiveMode;
use std::{path::PathBuf, time::Duration};

pub(super) const REVIEW_DEFAULT_WIDTH: f32 = 520.0;
const REVIEW_MIN_WIDTH: f32 = 300.0;
const REVIEW_MAX_SHARE: f32 = 0.6;

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

    /// The top bar's `+N −M` button, as Warp's; hidden outside a repository or with
    /// nothing changed, and when Settings turn it off.
    pub(super) fn render_diff_button(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (_, summary) = self.repo.as_ref()?;
        if summary.status.entries.is_empty() || !self.settings.flag("code-review-button") {
            return None;
        }
        let theme = self.theme();
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
                .when(self.review_open, |this| this.bg(hsla(theme.tab_active)))
                .hover(|style| style.bg(hsla(theme.tab_hover)))
                .on_click(cx.listener(|view, _, window, cx| {
                    cx.stop_propagation();
                    view.toggle_review(window, cx)
                }))
                .child(div().text_color(hsla(theme.text_muted)).child("±"))
                .child(review::counts(theme, summary.added, summary.removed)),
        )
    }

    pub(super) fn review_shown_width(&self, window: &Window) -> f32 {
        column_width(
            self.review_width,
            window,
            REVIEW_MAX_SHARE,
            REVIEW_MIN_WIDTH,
        )
    }

    pub(super) fn render_review(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        div()
            .flex()
            .flex_none()
            .h_full()
            .child(
                // Drag handle on the panel's left edge.
                div()
                    .flex_none()
                    .w(px(5.0))
                    .h_full()
                    .flex()
                    .justify_end()
                    .bg(hsla(theme.terminal_background))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .child(div().w(px(1.0)).h_full().bg(hsla(theme.border)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| {
                            cx.stop_propagation();
                            view.resizing_review = true;
                        }),
                    ),
            )
            .child(div().w(px(width)).h_full().child(self.review.clone()))
    }

    pub(super) fn drag_review_edge(&mut self, event: &MouseMoveEvent, window: &Window) -> bool {
        if !self.resizing_review {
            return false;
        }
        if !event.dragging() {
            self.resizing_review = false;
            return true;
        }
        let right = f32::from(window.viewport_size().width);
        self.review_width = column_width(
            right - f32::from(event.position.x),
            window,
            REVIEW_MAX_SHARE,
            REVIEW_MIN_WIDTH,
        );
        true
    }
}
