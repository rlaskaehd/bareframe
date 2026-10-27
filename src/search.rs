//! Global search: ripgrep's library over the explorer's roots, streamed into a tree of
//! files and matching lines, as in Warp's tools panel.
use crate::{
    explorer::file_name,
    text_field::{TextField, TextFieldEvent},
    theme::{self, Theme, hsla},
};
use futures::StreamExt;
use gpui::{
    Context, Entity, EventEmitter, HighlightStyle, StyledText, Subscription, Task,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{BinaryDetection, SearcherBuilder, sinks::Lossy},
};
use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Warp stops at 20,000 matches; past that the list is not useful anyway.
const MAX_MATCHES: usize = 20_000;
/// Lines longer than this are skipped, like Warp's 64KB rule.
const MAX_LINE: usize = 64 * 1024;
const ROW_HEIGHT: f32 = 24.0;
/// Characters kept before the first match when a line is shortened for display.
const CONTEXT: usize = 24;
const SNIPPET: usize = 160;

#[derive(Clone, Debug, PartialEq)]
struct LineMatch {
    line: u64,
    text: String,
    /// Byte ranges of the matches within `text`.
    ranges: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
struct FileMatches {
    root: PathBuf,
    path: PathBuf,
    lines: Vec<LineMatch>,
}

fn matcher(query: &str, case_sensitive: bool, regex: bool) -> Result<RegexMatcher, String> {
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .fixed_strings(!regex)
        .build(query)
        .map_err(|error| error.to_string())
}

/// Searches every non-ignored file under `roots`, sending each file with matches as soon as
/// it is done. Stops when `cancel` is set or after [`MAX_MATCHES`].
// ponytail: walks one thread; use `WalkBuilder::build_parallel` if huge trees feel slow.
// Cancel is seen between files and at each matching line, so a huge file with no matches
// is still read to the end; wrap the reader to check it if that shows up.
fn search(
    roots: &[PathBuf],
    matcher: &RegexMatcher,
    cancel: &AtomicBool,
    mut found: impl FnMut(FileMatches),
) {
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();
    let mut total = 0;
    for root in roots {
        for entry in ignore::WalkBuilder::new(root)
            .build()
            .filter_map(Result::ok)
        {
            if cancel.load(Ordering::Relaxed) || total >= MAX_MATCHES {
                return;
            }
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let mut lines = Vec::new();
            let sink = Lossy(|line, text: &str| {
                let text = text.trim_end_matches(['\n', '\r']);
                if text.len() <= MAX_LINE {
                    let mut ranges = Vec::new();
                    let _ = matcher.find_iter(text.as_bytes(), |found| {
                        ranges.push(found.start()..found.end());
                        true
                    });
                    total += ranges.len().max(1);
                    lines.push(LineMatch {
                        line,
                        text: text.to_owned(),
                        ranges,
                    });
                }
                Ok(total < MAX_MATCHES && !cancel.load(Ordering::Relaxed))
            });
            // Unreadable files are skipped, as ripgrep does by default.
            if searcher.search_path(matcher, entry.path(), sink).is_ok() && !lines.is_empty() {
                found(FileMatches {
                    root: root.clone(),
                    path: entry.into_path(),
                    lines,
                });
            }
        }
    }
}

/// The part of a line shown in the list: leading space dropped and, for long lines, a
/// window starting a little before the first match. Ranges are shifted to match.
fn snippet(text: &str, ranges: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let trimmed = text.len() - text.trim_start().len();
    let first = ranges.first().map_or(trimmed, |range| range.start);
    let mut start = trimmed.max(first.saturating_sub(CONTEXT)).min(text.len());
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + SNIPPET).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = if start > trimmed { "…" } else { "" };
    let shown = format!("{prefix}{}", &text[start..end]);
    let shift = |offset: usize| offset - start + prefix.len();
    let ranges = ranges
        .iter()
        .filter(|range| range.start >= start && range.end <= end)
        .map(|range| shift(range.start)..shift(range.end))
        .collect();
    (shown, ranges)
}

pub enum SearchEvent {
    /// A match was clicked: the file and its 1-based line.
    Open(PathBuf, u64),
    /// Esc in the search field: give focus back to the terminal.
    Dismiss,
}

#[derive(Clone)]
enum Row {
    Root(PathBuf, usize),
    File(PathBuf, PathBuf, usize, bool),
    /// Indices into `results` and that file's lines; rows are rebuilt every frame, so they
    /// don't copy the line text.
    Line(usize, usize),
}

pub struct SearchPanel {
    input: Entity<TextField>,
    query: String,
    case_sensitive: bool,
    regex: bool,
    roots: Vec<PathBuf>,
    results: Vec<FileMatches>,
    running: bool,
    error: Option<String>,
    collapsed: HashSet<PathBuf>,
    /// The current search's cancel flag and the task receiving its results.
    search: Option<(Arc<AtomicBool>, Task<()>)>,
    debounce: Option<Task<()>>,
    theme: &'static Theme,
    scroll: UniformListScrollHandle,
    _input: [Subscription; 2],
}

impl EventEmitter<SearchEvent> for SearchPanel {}

/// The search thread only stops when told to; the panel going away is such a time.
impl Drop for SearchPanel {
    fn drop(&mut self) {
        if let Some((cancel, _)) = &self.search {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl SearchPanel {
    pub fn new(theme: &'static Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextField::search(theme.accent, window, cx));
        let events = cx.subscribe(&input, |this, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Confirm(_) => this.start(cx),
            TextFieldEvent::Cancel => cx.emit(SearchEvent::Dismiss),
        });
        let changes = cx.observe(&input, |this, _, cx| this.schedule(cx));
        Self {
            input,
            query: String::new(),
            case_sensitive: false,
            regex: false,
            roots: Vec::new(),
            results: Vec::new(),
            running: false,
            error: None,
            collapsed: HashSet::new(),
            search: None,
            debounce: None,
            theme,
            scroll: UniformListScrollHandle::new(),
            _input: [events, changes],
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn set_roots(&mut self, roots: Vec<PathBuf>, cx: &mut Context<Self>) {
        if roots != self.roots {
            self.roots = roots;
            self.start(cx);
        }
    }

    /// Typing restarts the search 300ms after the last keystroke.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        if self.input.read(cx).text() == self.query {
            return;
        }
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = this.update(cx, |this, cx| this.start(cx));
        }));
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.debounce = None;
        if let Some((cancel, _)) = self.search.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.query = self.input.read(cx).text().to_owned();
        self.results.clear();
        self.collapsed.clear();
        self.error = None;
        self.running = false;
        cx.notify();
        if self.query.is_empty() || self.roots.is_empty() {
            return;
        }
        let matcher = match matcher(&self.query, self.case_sensitive, self.regex) {
            Ok(matcher) => matcher,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, mut receiver) = futures::channel::mpsc::unbounded();
        let roots = self.roots.clone();
        let flag = cancel.clone();
        std::thread::spawn(move || {
            search(&roots, &matcher, &flag, |file| {
                let _ = sender.unbounded_send(file);
            })
        });
        self.running = true;
        let task = cx.spawn(async move |this, cx| {
            while let Some(file) = receiver.next().await {
                let mut batch = vec![file];
                while let Ok(file) = receiver.try_recv() {
                    batch.push(file);
                }
                if this
                    .update(cx, |this, cx| {
                        this.results.extend(batch);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.running = false;
                cx.notify();
            });
        });
        self.search = Some((cancel, task));
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for root in &self.roots {
            let files: Vec<(usize, &FileMatches)> = self
                .results
                .iter()
                .enumerate()
                .filter(|(_, file)| &file.root == root)
                .collect();
            if files.is_empty() {
                continue;
            }
            let count = files.iter().map(|(_, file)| file.lines.len()).sum();
            rows.push(Row::Root(root.clone(), count));
            if self.collapsed.contains(root) {
                continue;
            }
            for (index, file) in files {
                let collapsed = self.collapsed.contains(&file.path);
                rows.push(Row::File(
                    file.path.clone(),
                    root.clone(),
                    file.lines.len(),
                    collapsed,
                ));
                if !collapsed {
                    rows.extend((0..file.lines.len()).map(|line| Row::Line(index, line)));
                }
            }
        }
        rows
    }

    fn toggle(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_path_buf());
        }
        cx.notify();
    }

    fn render_row(&self, row: Row, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = self.theme;
        let base = div()
            .id(("search-row", index))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pr_2()
            .rounded_sm()
            .hover(|style| style.bg(hsla(theme.tab_hover)));
        match row {
            Row::Root(path, count) => {
                let collapsed = self.collapsed.contains(&path);
                let name = file_name(&path);
                let toggled = path.clone();
                base.pl(px(4.0))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggled, cx)))
                    .child(theme::chevron(theme, !collapsed))
                    .child(theme::icon(theme, "icons/folder.svg"))
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .child(badge(theme, count))
                    .into_any_element()
            }
            Row::File(path, root, count, collapsed) => {
                let name = file_name(&path);
                let dir = path
                    .parent()
                    .and_then(|parent| parent.strip_prefix(&root).ok())
                    .map(|dir| dir.display().to_string())
                    .unwrap_or_default();
                let toggled = path.clone();
                base.pl(px(16.0))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggled, cx)))
                    .child(theme::chevron(theme, !collapsed))
                    .child(theme::icon(theme, "icons/file.svg"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap_1()
                            .child(div().flex_none().child(name))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(hsla(theme.text_muted))
                                    .child(dir),
                            ),
                    )
                    .child(badge(theme, count))
                    .into_any_element()
            }
            Row::Line(file, line) => {
                let file = &self.results[file];
                let (path, line) = (file.path.clone(), &file.lines[line]);
                let (text, ranges) = snippet(&line.text, &line.ranges);
                let highlight = HighlightStyle {
                    background_color: Some(hsla(theme.accent).opacity(0.35)),
                    color: Some(hsla(theme.text)),
                    ..Default::default()
                };
                let number = line.line;
                base.pl(px(44.0))
                    .text_color(hsla(theme.text_muted))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SearchEvent::Open(path.clone(), number))
                    }))
                    .child(
                        div().min_w_0().truncate().child(
                            StyledText::new(text).with_highlights(
                                ranges.into_iter().map(|range| (range, highlight)),
                            ),
                        ),
                    )
                    .into_any_element()
            }
        }
    }

    fn status(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        let count: usize = self.results.iter().map(|file| file.lines.len()).sum();
        match (self.running, count) {
            (true, 0) => "Searching…".into(),
            (false, 0) if !self.query.is_empty() => "No results".into(),
            (_, 0) => String::new(),
            (running, count) => format!(
                "{count} result{} in {} file{}{}",
                plural(count),
                self.results.len(),
                plural(self.results.len()),
                if running { "…" } else { "" }
            ),
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn badge(theme: &Theme, count: usize) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(5.0))
        .rounded_sm()
        .bg(hsla(theme.tab_active))
        .text_size(px(11.0))
        .text_color(hsla(theme.text_muted))
        .child(count.to_string())
}

fn toggle_button(
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    on: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(5.0))
        .rounded_sm()
        .text_size(px(11.5))
        .text_color(hsla(if on { theme.chrome } else { theme.text_muted }))
        .when(on, |this| this.bg(hsla(theme.accent)))
        .when(!on, |this| {
            this.hover(|style| style.bg(hsla(theme.tab_hover)))
        })
        .child(label)
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let rows = self.rows();
        let count = rows.len();
        let empty = self.input.read(cx).text().is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(theme::UI_FONT)
            .text_size(px(13.0))
            .text_color(hsla(theme.text))
            .child(
                div()
                    .id("search-field")
                    .flex_none()
                    .mx_2()
                    .mt_1()
                    .px_2()
                    .h(px(30.0))
                    .cursor_text()
                    // The text field is one line tall; the whole box takes the click.
                    .on_click(cx.listener(|this, _, window, cx| this.focus(window, cx)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .border_1()
                    .border_color(hsla(theme.border))
                    .bg(hsla(theme.terminal_background))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .when(empty, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .text_color(hsla(theme.text_muted))
                                        .child("Search in files"),
                                )
                            })
                            .child(self.input.clone()),
                    )
                    .child(
                        toggle_button(theme, "search-case", "Aa", self.case_sensitive).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.case_sensitive = !this.case_sensitive;
                                this.start(cx)
                            }),
                        ),
                    )
                    .child(
                        toggle_button(theme, "search-regex", ".*", self.regex).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.regex = !this.regex;
                                this.start(cx)
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .py_1()
                    .text_size(px(11.5))
                    .text_color(hsla(if self.error.is_some() {
                        theme.group_colors[4]
                    } else {
                        theme.text_muted
                    }))
                    .child(self.status()),
            )
            .child(
                uniform_list(
                    "search-rows",
                    count,
                    cx.processor(move |this, range: Range<usize>, _, cx| {
                        let rows = this.rows();
                        let end = range.end.min(rows.len());
                        let start = range.start.min(end);
                        rows[start..end]
                            .iter()
                            .cloned()
                            .enumerate()
                            .map(|(offset, row)| this.render_row(row, start + offset, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .px_1(),
            )
    }
}

#[cfg(test)]
// One-element range lists are the data here, not a mistaken `0..n`.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    #[test]
    fn search_respects_options_and_gitignore() {
        let dir = std::env::temp_dir().join(format!("bareframe-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(
            dir.join("a.rs"),
            "fn set_frozen() {}\nlet Set_Frozen = 1;\n",
        )
        .unwrap();
        std::fs::write(dir.join("target/b.rs"), "set_frozen\n").unwrap();
        std::fs::write(dir.join("bin"), b"set_frozen\0").unwrap();
        let run = |query: &str, case, regex| {
            let mut files = Vec::new();
            let matcher = matcher(query, case, regex).unwrap();
            search(
                std::slice::from_ref(&dir),
                &matcher,
                &AtomicBool::new(false),
                |file| files.push(file),
            );
            files
        };
        let found = run("set_frozen", false, false);
        assert_eq!(found.len(), 1, "ignored and binary files are skipped");
        assert_eq!(found[0].lines.len(), 2);
        assert_eq!(found[0].lines[0].line, 1);
        assert_eq!(found[0].lines[0].ranges, [3..13]);
        assert_eq!(run("set_frozen", true, false)[0].lines.len(), 1);
        assert!(
            run("set.frozen", false, false).is_empty(),
            "plain text by default"
        );
        assert_eq!(run("set.frozen", false, true)[0].lines.len(), 2);
        assert!(matcher("(", false, true).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn snippets_keep_matches_in_view() {
        assert_eq!(
            snippet("    foo(bar)", &[8..11]),
            ("foo(bar)".into(), vec![4..7])
        );
        let long = format!("{}needle{}", "x".repeat(100), "y".repeat(300));
        let (text, ranges) = snippet(&long, &[100..106]);
        assert!(text.starts_with('…'));
        assert_eq!(&text[ranges[0].clone()], "needle");
        let korean = format!("{}검색어", "가".repeat(40));
        let start = korean.find("검색어").unwrap();
        let (text, ranges) = snippet(&korean, &[start..start + "검색어".len()]);
        assert_eq!(&text[ranges[0].clone()], "검색어");
    }
}
