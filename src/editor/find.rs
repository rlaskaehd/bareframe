//! Find and replace, and go to line: the bars above the text and what they do to it.
use super::EditorPane;
use crate::{
    text_field::{TextField, TextFieldEvent},
    theme::{self, hsla},
    widgets,
};
use bareframe_core::text::Position;
use gpui::{App, Context, Entity, Subscription, Window, div, prelude::*, px};
use std::ops::Range;

/// The find bar: the query, an optional replacement, and where the query occurs.
pub(super) struct FindBar {
    query: Entity<TextField>,
    replacement: Option<Entity<TextField>>,
    matches: Vec<Range<Position>>,
    current: Option<usize>,
    /// The text and buffer version `matches` were found for.
    found: Option<(String, u64)>,
    _subscriptions: Vec<Subscription>,
}

impl EditorPane {
    /// Find matches on `lines`, the index of the first of them among all matches, and
    /// the current match's index.
    pub(super) fn matches_in(
        &self,
        lines: Range<usize>,
    ) -> (Vec<Range<Position>>, usize, Option<usize>) {
        let Some(find) = &self.find else {
            return (Vec::new(), 0, None);
        };
        let low = find
            .matches
            .partition_point(|range| range.start.line < lines.start);
        let high = find
            .matches
            .partition_point(|range| range.start.line < lines.end);
        (find.matches[low..high].to_vec(), low, find.current)
    }

    pub(super) fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.go_to = None;
        let accent = self.theme.accent;
        // A selection on one line becomes the query, as in most editors.
        let selected = self.buffer.slice(self.selection());
        let seed = (!selected.is_empty() && !selected.contains('\n')).then_some(selected);
        if self.find.is_none() {
            let query = cx.new(|cx| TextField::search(accent, window, cx));
            let events = cx.subscribe_in(&query, window, |e, _, event, window, cx| match event {
                TextFieldEvent::Confirm(_) => e.find_step(true, cx),
                TextFieldEvent::Cancel => e.close_bars(window, cx),
            });
            // Typing in the field jumps to the first match from where the search began.
            let changes = cx.observe(&query, |e, _, cx| {
                if let Some(find) = &mut e.find {
                    find.found = None;
                }
                let from = e.selection().start;
                e.anchor = from;
                e.head = from;
                e.find_from(from, cx);
            });
            self.find = Some(FindBar {
                query,
                replacement: None,
                matches: Vec::new(),
                current: None,
                found: None,
                _subscriptions: vec![events, changes],
            });
        }
        let Some(find) = &mut self.find else {
            return;
        };
        if let Some(seed) = seed {
            find.query.update(cx, |input, cx| input.set_text(&seed, cx));
        }
        if replace && find.replacement.is_none() {
            let input = cx.new(|cx| TextField::search(accent, window, cx));
            let events = cx.subscribe_in(&input, window, |e, _, event, window, cx| match event {
                TextFieldEvent::Confirm(_) => e.replace_current(cx),
                TextFieldEvent::Cancel => e.close_bars(window, cx),
            });
            find._subscriptions.push(events);
            find.replacement = Some(input);
        }
        find.query.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn open_go_to(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        let input = cx.new(|cx| TextField::search(self.theme.accent, window, cx));
        let events = cx.subscribe_in(&input, window, |e, _, event, window, cx| {
            if let TextFieldEvent::Confirm(text) = event
                && let Ok(line) = text.trim().parse::<u64>()
            {
                e.go_to_line(line.max(1), cx);
            }
            e.close_bars(window, cx);
        });
        self.go_to = Some((input, events));
        cx.notify();
    }

    pub(super) fn close_bars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.go_to = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Finds the query again if it or the text changed since the last look.
    pub(super) fn refresh_find(&mut self, cx: &App) {
        let Some(find) = &mut self.find else {
            return;
        };
        let query = find.query.read(cx).text().to_owned();
        let key = (query, self.buffer.version);
        if find.found.as_ref() != Some(&key) {
            find.matches = self.buffer.find_all(&key.0);
            find.current = None;
            find.found = Some(key);
        }
    }

    fn select_match(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(range) = self
            .find
            .as_ref()
            .and_then(|find| find.matches.get(index))
            .cloned()
        else {
            return;
        };
        if let Some(find) = &mut self.find {
            find.current = Some(index);
        }
        self.anchor = range.start;
        self.select(range.end, true, cx);
    }

    /// The first match at or after `from`, wrapping to the top.
    fn find_from(&mut self, from: Position, cx: &mut Context<Self>) {
        self.refresh_find(cx);
        let Some(find) = &self.find else {
            return;
        };
        if find.matches.is_empty() {
            return cx.notify();
        }
        let index = find.matches.partition_point(|range| range.start < from) % find.matches.len();
        self.select_match(index, cx);
    }

    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.refresh_find(cx);
        let Some(find) = &self.find else {
            return;
        };
        let count = find.matches.len();
        if count == 0 {
            return;
        }
        let index = if forward {
            find.matches
                .partition_point(|range| range.start < self.selection().end)
                % count
        } else {
            let before = find
                .matches
                .partition_point(|range| range.start < self.selection().start);
            (before + count - 1) % count
        };
        self.select_match(index, cx);
    }

    fn replacement_text(&self, cx: &App) -> Option<String> {
        let find = self.find.as_ref()?;
        Some(find.replacement.as_ref()?.read(cx).text().to_owned())
    }

    /// Replaces the selected match and moves to the next one.
    fn replace_current(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.replacement_text(cx) else {
            return;
        };
        self.refresh_find(cx);
        let selection = self.selection();
        let selected_match = self
            .find
            .as_ref()
            .is_some_and(|find| find.matches.contains(&selection));
        if selected_match {
            self.replace(selection, &text, cx);
        }
        let from = self.head;
        self.find_from(from, cx);
    }

    /// Every match at once, as one step to undo.
    fn replace_all(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.replacement_text(cx) else {
            return;
        };
        self.refresh_find(cx);
        let Some(find) = &self.find else {
            return;
        };
        if find.matches.is_empty() || self.read_only {
            return;
        }
        let mut lines: Vec<String> = self.buffer.lines().map(str::to_owned).collect();
        for range in find.matches.iter().rev() {
            lines[range.start.line].replace_range(range.start.column..range.end.column, &text);
        }
        let head = self.head;
        let end = self.buffer.end();
        self.replace(Position::default()..end, &lines.join("\n"), cx);
        self.select(head, false, cx);
    }

    pub(super) fn render_bars(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let theme = self.theme;
        let field = |id: &'static str, input: &Entity<TextField>, hint: &'static str, cx: &App| {
            let empty = input.read(cx).text().is_empty();
            let focus_input = input.clone();
            div()
                .id(id)
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(24.0))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(hsla(theme.border))
                .bg(hsla(theme.terminal_background))
                .overflow_hidden()
                .cursor_text()
                .on_click(move |_, window, cx| {
                    focus_input.update(cx, |input, cx| input.focus(window, cx))
                })
                .when(empty, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(9.0))
                            .text_color(hsla(theme.text_muted))
                            .child(hint),
                    )
                })
                .child(input.clone())
        };
        let button = |id: &'static str, label: &'static str| {
            widgets::ghost_button(theme, id, label)
                .flex_none()
                .px_2()
                .py(px(2.0))
                .rounded_md()
        };
        let bar = div()
            .flex()
            .flex_none()
            .flex_col()
            .gap_1()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(hsla(theme.border))
            .bg(hsla(theme.tab_active))
            .font_family(theme::UI_FONT)
            .text_size(px(12.0));
        if let Some((input, _)) = &self.go_to {
            return Some(
                bar.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child("Go to line")
                        .child(field("editor-go-to", input, "Line number", cx)),
                )
                .into_any_element(),
            );
        }
        let find = self.find.as_ref()?;
        let count = match (find.current, find.matches.len()) {
            (_, 0) if find.query.read(cx).text().is_empty() => String::new(),
            (_, 0) => "No results".to_owned(),
            (Some(current), total) => format!("{} of {total}", current + 1),
            (None, total) => format!("{total} found"),
        };
        Some(
            bar.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(field("editor-find", &find.query, "Find", cx))
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(64.0))
                            .text_color(hsla(theme.text_muted))
                            .child(count),
                    )
                    .child(
                        button("editor-find-previous", "↑")
                            .on_click(cx.listener(|e, _, _, cx| e.find_step(false, cx))),
                    )
                    .child(
                        button("editor-find-next", "↓")
                            .on_click(cx.listener(|e, _, _, cx| e.find_step(true, cx))),
                    )
                    .child(
                        button("editor-find-close", "×")
                            .on_click(cx.listener(|e, _, window, cx| e.close_bars(window, cx))),
                    ),
            )
            .when_some(find.replacement.as_ref(), |this, replacement| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(field("editor-replace", replacement, "Replace", cx))
                        .child(
                            button("editor-replace-one", "Replace")
                                .on_click(cx.listener(|e, _, _, cx| e.replace_current(cx))),
                        )
                        .child(
                            button("editor-replace-all", "All")
                                .on_click(cx.listener(|e, _, _, cx| e.replace_all(cx))),
                        ),
                )
            })
            .into_any_element(),
        )
    }
}
