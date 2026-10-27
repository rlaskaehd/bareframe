//! Warp's code editor, first step: a text file in a pane with a cursor and selection, the
//! usual movement and editing keys, undo, clipboard, Korean input through the system
//! input method, saving, and guarding unsaved changes. Syntax colors and find come next.
use crate::{
    rename::{RenameEvent, RenameInput},
    syntax::{self, LineSyntax, Syntax},
    theme::{self, Theme, hsla},
    watch::{Watch, watch_file},
};
use bareframe_core::text::{Buffer, LineEnding, Position};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, GlobalElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollWheelEvent, ShapedLine, Style,
    Subscription, Task, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point,
    prelude::*, px, relative, size,
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

actions!(
    editor,
    [
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocumentStart,
        DocumentEnd,
        SelectDocumentStart,
        SelectDocumentEnd,
        PageUp,
        PageDown,
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteLineLeft,
        Newline,
        Tab,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Save,
        Collapse,
        Find,
        FindReplace,
        FindNext,
        FindPrevious,
        GoToLine,
    ]
);

/// Editing keys apply only to the text area, so the find field keeps Tab and arrows.
const KEY_CONTEXT: &str = "Editor";
/// Find, go to line and save apply anywhere in the editor pane, find field included.
const FILE_CONTEXT: &str = "EditorFile";
const MARKDOWN_FILE_CONTEXT: &str = "EditorFile Markdown";
/// Files larger than this open read-only: every edit copies a line vector this long.
const MAX_EDITABLE: u64 = 5 << 20;
const LINE_HEIGHT: f32 = 1.45;

pub enum EditorEvent {
    /// The text or its saved state changed; titles show a dot for unsaved changes.
    Changed,
    /// The user chose Save or Don't Save when asked; the pane may go.
    Close,
    /// A Markdown file's `Rendered` view was asked for, and nothing unsaved is in the way.
    ShowRendered,
}

#[derive(Clone, Copy, PartialEq)]
pub enum LineNumbers {
    Absolute,
    Relative,
    Off,
}

impl LineNumbers {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "relative" => Self::Relative,
            "off" => Self::Off,
            _ => Self::Absolute,
        }
    }
}

/// The find bar: the query, an optional replacement, and where the query occurs.
struct FindBar {
    query: Entity<RenameInput>,
    replacement: Option<Entity<RenameInput>>,
    matches: Vec<Range<Position>>,
    current: Option<usize>,
    /// The text and buffer version `matches` were found for.
    found: Option<(String, u64)>,
    _subscriptions: Vec<Subscription>,
}

enum Notice {
    ConfirmClose,
    ChangedOnDisk,
    Error(String),
}

/// What the last paint laid out, for clicks, input method boxes and Up/Down.
struct Layout {
    bounds: Bounds<Pixels>,
    /// Where column 0 is drawn when not scrolled sideways.
    text_left: Pixels,
    line_height: Pixels,
    first: usize,
    lines: Vec<ShapedLine>,
}

pub struct EditorPane {
    path: PathBuf,
    buffer: Buffer,
    anchor: Position,
    head: Position,
    /// The input method's text still being composed.
    marked: Option<Range<Position>>,
    /// Up and Down keep this x across shorter lines.
    goal_x: Option<Pixels>,
    scroll: Point<Pixels>,
    reveal: bool,
    selecting: bool,
    focus: FocusHandle,
    theme: &'static Theme,
    font: String,
    font_size: f32,
    read_only: bool,
    notice: Option<Notice>,
    /// The file as last read or written, so our own save isn't taken for an outside edit.
    disk: String,
    /// "Don't Save" was chosen: closing no longer asks.
    discarded: bool,
    /// The question about unsaved changes was asked to switch to `Rendered`, not to close.
    to_rendered: bool,
    layout: Option<Layout>,
    /// The grammar for this file's colors, if one is bundled.
    language: Option<&'static str>,
    find: Option<FindBar>,
    go_to: Option<(Entity<RenameInput>, Subscription)>,
    line_numbers: LineNumbers,
    autosave: bool,
    _autosave: Option<Task<()>>,
    /// Colors per line from the last parse; lines edited since have none until the next.
    syntax: Vec<LineSyntax>,
    _highlight: Option<Task<()>>,
    _watch: Option<Watch>,
}

impl EventEmitter<EditorEvent> for EditorPane {}

/// Whether bareframe's editor can open `path`: a regular file whose start reads as UTF-8
/// text without NUL bytes.
pub fn is_text_file(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let mut head = Vec::new();
    if file.take(8192).read_to_end(&mut head).is_err() || head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(&head) {
        Ok(_) => true,
        // Cut in the middle of a character at the 8 KB mark.
        Err(error) => error.error_len().is_none(),
    }
}

impl EditorPane {
    pub fn new(
        path: PathBuf,
        theme: &'static Theme,
        font: String,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let size = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());
        let (text, error) = match std::fs::read_to_string(&path) {
            Ok(text) => (text, None),
            Err(error) => (String::new(), Some(error.to_string())),
        };
        let mut editor = Self {
            buffer: Buffer::new(&text),
            disk: text,
            path,
            anchor: Position::default(),
            head: Position::default(),
            marked: None,
            goal_x: None,
            scroll: Point::default(),
            reveal: false,
            selecting: false,
            focus: cx.focus_handle(),
            theme,
            font,
            font_size,
            read_only: size > MAX_EDITABLE || error.is_some(),
            notice: error.map(Notice::Error),
            discarded: false,
            to_rendered: false,
            layout: None,
            language: None,
            find: None,
            go_to: None,
            line_numbers: LineNumbers::Absolute,
            autosave: false,
            _autosave: None,
            syntax: Vec::new(),
            _highlight: None,
            _watch: None,
        };
        editor.language = syntax::language_for(&editor.path);
        editor._watch = editor.watch(cx);
        editor.highlight(cx);
        editor
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> String {
        let name = self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into(),
        );
        if self.buffer.is_dirty() {
            format!("{name} ●")
        } else {
            name
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn set_style(
        &mut self,
        theme: &'static Theme,
        font: String,
        font_size: f32,
        cx: &mut Context<Self>,
    ) {
        self.theme = theme;
        self.font = font;
        self.font_size = font_size;
        cx.notify();
    }

    pub fn set_options(
        &mut self,
        line_numbers: LineNumbers,
        autosave: bool,
        cx: &mut Context<Self>,
    ) {
        self.line_numbers = line_numbers;
        self.autosave = autosave;
        cx.notify();
    }

    /// Whether the pane may close now; if not, the editor asks the user first.
    pub fn request_close(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.buffer.is_dirty() || self.discarded {
            return true;
        }
        self.to_rendered = false;
        self.notice = Some(Notice::ConfirmClose);
        cx.notify();
        false
    }

    /// `Rendered` for a Markdown file: straight away, or once unsaved changes are settled.
    fn show_rendered(&mut self, cx: &mut Context<Self>) {
        if !crate::markdown::is_markdown(&self.path) {
            return;
        }
        if self.buffer.is_dirty() && !self.discarded {
            self.to_rendered = true;
            self.notice = Some(Notice::ConfirmClose);
            cx.notify();
        } else {
            cx.emit(EditorEvent::ShowRendered);
        }
    }

    /// After Save or Don't Save: on to what the question was asked for.
    fn settled(&mut self, cx: &mut Context<Self>) {
        cx.emit(if std::mem::take(&mut self.to_rendered) {
            EditorEvent::ShowRendered
        } else {
            EditorEvent::Close
        });
    }

    /// Moves the cursor to a 1-based line, as a search result asks.
    pub fn go_to_line(&mut self, line: u64, cx: &mut Context<Self>) {
        let line = (line.saturating_sub(1) as usize).min(self.buffer.line_count() - 1);
        self.select(Position::new(line, 0), false, cx);
        // Put the line a third of the way down rather than at the very edge.
        if let Some(layout) = &self.layout {
            let visible = layout.bounds.size.height / layout.line_height;
            let top = (line as f32 - visible / 3.0).max(0.0);
            self.scroll.y = layout.line_height * top;
        }
    }

    fn selection(&self) -> Range<Position> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    fn select(&mut self, head: Position, extend: bool, cx: &mut Context<Self>) {
        self.head = self.buffer.clamp(head);
        if !extend {
            self.anchor = self.head;
        }
        self.marked = None;
        self.reveal = true;
        cx.notify();
    }

    /// Replaces the selection (or the text being composed) with `text`.
    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = self.marked.clone().unwrap_or_else(|| self.selection());
        self.replace(range, text, cx);
    }

    fn replace(&mut self, range: Range<Position>, text: &str, cx: &mut Context<Self>) -> Position {
        if self.read_only {
            return range.start;
        }
        let was_dirty = self.buffer.is_dirty();
        let (first, last) = (
            range.start.min(range.end).line,
            range.start.max(range.end).line,
        );
        let end = self.buffer.replace(range, text, Instant::now());
        // The edited lines lose their colors until the next parse; the others keep theirs.
        if last < self.syntax.len() {
            let fresh = end.line - first + 1;
            self.syntax
                .splice(first..=last, std::iter::repeat_n(Vec::new(), fresh));
        }
        self.highlight(cx);
        if self.autosave {
            self._autosave = Some(cx.spawn(async move |editor, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                editor
                    .update(cx, |editor, cx| {
                        if editor.buffer.is_dirty() {
                            editor.save(cx);
                        }
                    })
                    .ok();
            }));
        }
        self.anchor = end;
        self.head = end;
        self.marked = None;
        self.goal_x = None;
        self.reveal = true;
        self.discarded = false;
        if was_dirty != self.buffer.is_dirty() || !was_dirty {
            cx.emit(EditorEvent::Changed);
        }
        cx.notify();
        end
    }

    /// With nothing selected, `step` picks what to delete: the char or word before, etc.
    fn delete_with(
        &mut self,
        step: impl Fn(&Buffer, Position) -> Position,
        cx: &mut Context<Self>,
    ) {
        let range = if self.anchor == self.head {
            let other = step(&self.buffer, self.head);
            other.min(self.head)..other.max(self.head)
        } else {
            self.selection()
        };
        self.replace(range, "", cx);
    }

    fn line_height(&self) -> Pixels {
        px((self.font_size * LINE_HEIGHT).round())
    }

    fn base_font(&self) -> gpui::Font {
        gpui::font(self.font.clone())
    }

    fn shape(&self, line: usize, window: &mut Window) -> ShapedLine {
        let text = self.buffer.line(line).to_owned();
        let base = TextRun {
            len: 0,
            font: self.base_font(),
            color: hsla(self.theme.terminal_foreground),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Colors from a parse of older text may no longer fit; those are dropped.
        let colors: Vec<(Range<usize>, gpui::Hsla)> = self
            .syntax
            .get(line)
            .into_iter()
            .flatten()
            .filter(|(range, _)| {
                range.end <= text.len()
                    && text.is_char_boundary(range.start)
                    && text.is_char_boundary(range.end)
            })
            .map(|(range, syntax)| (range.clone(), hsla(syntax_color(self.theme, *syntax))))
            .collect();
        let marked = self
            .marked
            .as_ref()
            .filter(|marked| marked.start.line <= line && line <= marked.end.line)
            .map(|marked| {
                let start = if marked.start.line == line {
                    marked.start.column
                } else {
                    0
                };
                let end = if marked.end.line == line {
                    marked.end.column
                } else {
                    text.len()
                };
                start..end
            });
        let mut cuts = vec![0, text.len()];
        cuts.extend(
            colors
                .iter()
                .flat_map(|(range, _)| [range.start, range.end]),
        );
        cuts.extend(marked.iter().flat_map(|range| [range.start, range.end]));
        cuts.sort_unstable();
        cuts.dedup();
        let mut runs: Vec<TextRun> = cuts
            .windows(2)
            .map(|pair| {
                let (at, len) = (pair[0], pair[1] - pair[0]);
                let color = colors
                    .iter()
                    .find(|(range, _)| range.contains(&at))
                    .map_or(base.color, |(_, color)| *color);
                let underline = marked
                    .as_ref()
                    .filter(|range| range.contains(&at))
                    .map(|_| UnderlineStyle {
                        color: Some(color),
                        thickness: px(1.0),
                        wavy: false,
                    });
                TextRun {
                    len,
                    color,
                    underline,
                    ..base.clone()
                }
            })
            .collect();
        if runs.is_empty() {
            runs.push(base);
        }
        window
            .text_system()
            .shape_line(text.into(), px(self.font_size), &runs, None)
    }

    /// Parses the file again in the background a moment after the last change.
    fn highlight(&mut self, cx: &mut Context<Self>) {
        let Some(language) = self.language else {
            return;
        };
        // Joined with plain line breaks, as the line vector counts them.
        let text = self.buffer.lines().collect::<Vec<_>>().join("\n");
        if text.len() > syntax::MAX_HIGHLIGHT {
            self.syntax.clear();
            return;
        }
        self._highlight = Some(cx.spawn(async move |editor, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            let lines = cx
                .background_executor()
                .spawn(async move { syntax::highlight(language, &text) })
                .await;
            editor
                .update(cx, |editor, cx| {
                    if let Some(lines) = lines {
                        editor.syntax = lines;
                        cx.notify();
                    }
                })
                .ok();
        }));
    }

    fn x_of(&self, position: Position, window: &mut Window) -> Pixels {
        self.shape(position.line, window)
            .x_for_index(position.column)
    }

    fn vertical(
        &mut self,
        lines: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let goal = match self.goal_x {
            Some(x) => x,
            None => self.x_of(self.head, window),
        };
        let target = self.head.line as isize + lines;
        let head = if target < 0 {
            Position::new(0, 0)
        } else if target as usize >= self.buffer.line_count() {
            self.buffer.end()
        } else {
            let line = target as usize;
            Position::new(line, self.shape(line, window).closest_index_for_x(goal))
        };
        self.select(head, extend, cx);
        self.goal_x = Some(goal);
    }

    fn page(&self) -> isize {
        self.layout.as_ref().map_or(30, |layout| {
            ((layout.bounds.size.height / layout.line_height) as isize - 2).max(1)
        })
    }

    fn position_for(&self, at: Point<Pixels>) -> Option<Position> {
        let layout = self.layout.as_ref()?;
        let y = at.y - layout.bounds.top() + self.scroll.y;
        let line =
            ((y / layout.line_height).floor().max(0.0) as usize).min(self.buffer.line_count() - 1);
        let shaped = layout
            .lines
            .get(line.checked_sub(layout.first)?)
            .or(layout.lines.last())?;
        let x = at.x - layout.text_left + self.scroll.x;
        let column = if line < layout.first + layout.lines.len() {
            shaped.closest_index_for_x(x)
        } else {
            self.buffer.line(line).len()
        };
        Some(self.buffer.clamp(Position::new(line, column)))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        let Some(position) = self.position_for(event.position) else {
            return;
        };
        match event.click_count {
            2 => {
                let word = self.buffer.word_at(position);
                self.anchor = word.start;
                self.select(word.end, true, cx);
            }
            3 => {
                self.anchor = Position::new(position.line, 0);
                let end = self.buffer.next(Position::new(
                    position.line,
                    self.buffer.line(position.line).len(),
                ));
                self.select(end, true, cx);
            }
            _ => self.select(position, event.modifiers.shift, cx),
        }
        self.goal_x = None;
        self.selecting = true;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
            return;
        }
        if let Some(position) = self.position_for(event.position) {
            self.select(position, true, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.line_height());
        self.scroll.x = (self.scroll.x - delta.x).max(Pixels::ZERO);
        self.scroll.y = (self.scroll.y - delta.y).max(Pixels::ZERO);
        self.clamp_scroll();
        cx.notify();
    }

    fn clamp_scroll(&mut self) {
        let Some(layout) = &self.layout else {
            return;
        };
        let height = layout.line_height * self.buffer.line_count() as f32;
        let max_y =
            (height - layout.bounds.size.height + layout.line_height * 3.0).max(Pixels::ZERO);
        self.scroll.y = self.scroll.y.min(max_y);
        let widest = layout
            .lines
            .iter()
            .map(|line| line.width)
            .fold(Pixels::ZERO, Pixels::max);
        let room = layout.bounds.right() - layout.text_left;
        let max_x = (widest - room + layout.line_height * 2.0).max(Pixels::ZERO);
        self.scroll.x = self.scroll.x.min(max_x.max(self.scroll.x.min(max_x)));
    }

    fn save(&mut self, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        let text = self.buffer.text();
        // Written in place, so the file keeps its permissions, owner and hard links.
        match std::fs::write(&self.path, &text) {
            Ok(()) => {
                self.disk = text;
                self.buffer.mark_saved();
                self.notice = None;
                cx.emit(EditorEvent::Changed);
                cx.notify();
                true
            }
            Err(error) => {
                self.notice = Some(Notice::Error(format!("Could not save: {error}")));
                cx.notify();
                false
            }
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        self.buffer = Buffer::new(&text);
        self.disk = text;
        self.anchor = self.buffer.clamp(self.anchor);
        self.head = self.buffer.clamp(self.head);
        self.marked = None;
        self.notice = None;
        self.highlight(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn on_disk_change(&mut self, cx: &mut Context<Self>) {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        if text == self.disk {
            return;
        }
        if self.buffer.is_dirty() {
            self.notice = Some(Notice::ChangedOnDisk);
            cx.notify();
        } else {
            self.reload(cx);
        }
    }

    fn watch(&self, cx: &mut Context<Self>) -> Option<Watch> {
        watch_file(&self.path, Self::on_disk_change, cx)
    }

    fn indent_unit(&self) -> &'static str {
        let tabs = self
            .buffer
            .lines()
            .take(500)
            .any(|line| line.starts_with('\t'));
        if tabs { "\t" } else { "    " }
    }

    fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.go_to = None;
        let accent = self.theme.accent;
        // A selection on one line becomes the query, as in most editors.
        let selected = self.buffer.slice(self.selection());
        let seed = (!selected.is_empty() && !selected.contains('\n')).then_some(selected);
        if self.find.is_none() {
            let query = cx.new(|cx| RenameInput::search(accent, window, cx));
            let events = cx.subscribe_in(&query, window, |e, _, event, window, cx| match event {
                RenameEvent::Confirm(_) => e.find_step(true, cx),
                RenameEvent::Cancel => e.close_bars(window, cx),
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
            let input = cx.new(|cx| RenameInput::search(accent, window, cx));
            let events = cx.subscribe_in(&input, window, |e, _, event, window, cx| match event {
                RenameEvent::Confirm(_) => e.replace_current(cx),
                RenameEvent::Cancel => e.close_bars(window, cx),
            });
            find._subscriptions.push(events);
            find.replacement = Some(input);
        }
        find.query.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn open_go_to(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        let input = cx.new(|cx| RenameInput::search(self.theme.accent, window, cx));
        let events = cx.subscribe_in(&input, window, |e, _, event, window, cx| {
            if let RenameEvent::Confirm(text) = event
                && let Ok(line) = text.trim().parse::<u64>()
            {
                e.go_to_line(line.max(1), cx);
            }
            e.close_bars(window, cx);
        });
        self.go_to = Some((input, events));
        cx.notify();
    }

    fn close_bars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.go_to = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Finds the query again if it or the text changed since the last look.
    fn refresh_find(&mut self, cx: &App) {
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

    fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
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

    fn render_bars(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let theme = self.theme;
        let field =
            |id: &'static str, input: &Entity<RenameInput>, hint: &'static str, cx: &App| {
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
            div()
                .id(id)
                .flex_none()
                .px_2()
                .py(px(2.0))
                .rounded_md()
                .text_color(hsla(theme.text_muted))
                .hover(|style| style.bg(hsla(theme.tab_hover)).text_color(hsla(theme.text)))
                .child(label)
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

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let notice = self.notice.as_ref()?;
        let theme = self.theme;
        let name = self
            .path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_2()
                .py(px(2.0))
                .rounded_md()
                .border_1()
                .border_color(hsla(if primary { theme.accent } else { theme.border }))
                .when(primary, |this| {
                    this.bg(hsla(theme.accent)).text_color(hsla(theme.chrome))
                })
                .hover(|style| style.opacity(0.85))
                .child(label)
        };
        let (message, buttons): (String, Vec<gpui::Stateful<gpui::Div>>) = match notice {
            Notice::ConfirmClose => (
                if self.to_rendered {
                    format!("Save changes to {name} before showing it rendered?")
                } else {
                    format!("Save changes to {name} before closing?")
                },
                vec![
                    button("editor-close-cancel", "Cancel", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            editor.notice = None;
                            editor.to_rendered = false;
                            cx.notify();
                        },
                    )),
                    button("editor-close-discard", "Don't Save", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            editor.discarded = true;
                            editor.notice = None;
                            editor.settled(cx);
                        },
                    )),
                    button("editor-close-save", "Save", true).on_click(cx.listener(
                        |editor, _, _, cx| {
                            if editor.save(cx) {
                                editor.settled(cx);
                            }
                        },
                    )),
                ],
            ),
            Notice::ChangedOnDisk => (
                format!("{name} changed on disk."),
                vec![
                    button("editor-keep", "Keep my changes", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            // Saving now would overwrite the outside change, as asked.
                            editor.disk = std::fs::read_to_string(&editor.path).unwrap_or_default();
                            editor.notice = None;
                            cx.notify();
                        },
                    )),
                    button("editor-reload", "Reload", true)
                        .on_click(cx.listener(|editor, _, _, cx| editor.reload(cx))),
                ],
            ),
            Notice::Error(error) => (
                error.clone(),
                vec![
                    button("editor-dismiss", "Dismiss", false).on_click(cx.listener(
                        |editor, _, _, cx| {
                            editor.notice = None;
                            cx.notify();
                        },
                    )),
                ],
            ),
        };
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .border_b_1()
                .border_color(hsla(theme.border))
                .bg(hsla(theme.tab_active))
                .font_family(theme::UI_FONT)
                .text_size(px(12.0))
                .child(div().flex_1().min_w_0().truncate().child(message))
                .children(buttons),
        )
    }

    fn render_status(&self) -> impl IntoElement {
        let theme = self.theme;
        let line = self.buffer.line(self.head.line);
        let column = line[..self.head.column.min(line.len())].chars().count() + 1;
        let selected = self.buffer.slice(self.selection()).chars().count();
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
            .when(self.read_only, |this| this.child("Read-only"))
            .child(match self.buffer.line_ending {
                LineEnding::Lf => "LF",
                LineEnding::CrLf => "CRLF",
            })
            .child("UTF-8")
    }
}

impl EntityInputHandler for EditorPane {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let start = self.buffer.from_utf16(range_utf16.start);
        let end = self.buffer.from_utf16(range_utf16.end);
        actual_range.replace(self.buffer.to_utf16(start)..self.buffer.to_utf16(end));
        Some(self.buffer.slice(start..end))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let range = self.selection();
        Some(UTF16Selection {
            range: self.buffer.to_utf16(range.start)..self.buffer.to_utf16(range.end),
            reversed: self.head < self.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = self.marked.as_ref()?;
        Some(self.buffer.to_utf16(marked.start)..self.buffer.to_utf16(marked.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = match range_utf16 {
            Some(range) => self.buffer.from_utf16(range.start)..self.buffer.from_utf16(range.end),
            None => self.marked.clone().unwrap_or_else(|| self.selection()),
        };
        self.replace(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = match range_utf16 {
            Some(range) => self.buffer.from_utf16(range.start)..self.buffer.from_utf16(range.end),
            None => self.marked.clone().unwrap_or_else(|| self.selection()),
        };
        let start = range.start.min(range.end);
        let end = self.replace(range, new_text, cx);
        if self.read_only {
            return;
        }
        self.marked = (!new_text.is_empty()).then_some(start..end);
        if let Some(selected) = new_selected_range_utf16 {
            let base = self.buffer.to_utf16(start);
            self.anchor = self.buffer.from_utf16(base + selected.start);
            self.head = self.buffer.from_utf16(base + selected.end);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let start = self.buffer.from_utf16(range_utf16.start);
        let shaped = layout.lines.get(start.line.checked_sub(layout.first)?)?;
        let x = layout.text_left - self.scroll.x + shaped.x_for_index(start.column);
        let y = layout.bounds.top() + layout.line_height * start.line as f32 - self.scroll.y;
        Some(Bounds::new(point(x, y), size(px(2.0), layout.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let position = self.position_for(point)?;
        Some(self.buffer.to_utf16(position))
    }
}

struct EditorElement {
    editor: Entity<EditorPane>,
}

struct Prepaint {
    layout: Layout,
    gutter: Vec<(ShapedLine, Pixels)>,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.editor.update(cx, |editor, cx| {
            let line_height = editor.line_height();
            let font_id = window.text_system().resolve_font(&editor.base_font());
            let em = window
                .text_system()
                .em_advance(font_id, px(editor.font_size))
                .unwrap_or(px(8.0));
            let digits = editor.buffer.line_count().to_string().len().max(3);
            let gutter_width = match editor.line_numbers {
                LineNumbers::Off => Pixels::ZERO,
                _ => em * (digits as f32 + 2.0),
            };
            editor.refresh_find(cx);
            let text_left = bounds.left() + gutter_width + em;
            // Bring the cursor into view after a move or an edit.
            if std::mem::take(&mut editor.reveal) {
                let top = line_height * editor.head.line as f32;
                if top < editor.scroll.y {
                    editor.scroll.y = top;
                } else if top + line_height > editor.scroll.y + bounds.size.height {
                    editor.scroll.y = top + line_height - bounds.size.height;
                }
                let x = editor.x_of(editor.head, window);
                let room = bounds.right() - text_left - em * 2.0;
                if x < editor.scroll.x {
                    editor.scroll.x = (x - em * 4.0).max(Pixels::ZERO);
                } else if x > editor.scroll.x + room {
                    editor.scroll.x = x - room;
                }
            }
            let first = (editor.scroll.y / line_height).floor().max(0.0) as usize;
            let count = (bounds.size.height / line_height).ceil() as usize + 1;
            let last = (first + count).min(editor.buffer.line_count());
            let lines = (first..last)
                .map(|line| editor.shape(line, window))
                .collect();
            let number_runs = |len, color| {
                vec![TextRun {
                    len,
                    font: editor.base_font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }]
            };
            let numbers = if editor.line_numbers == LineNumbers::Off {
                0..0
            } else {
                first..last
            };
            let gutter = numbers
                .map(|line| {
                    // Relative numbers count from the cursor's line, which shows its own.
                    let label = match editor.line_numbers {
                        LineNumbers::Relative if line != editor.head.line => {
                            line.abs_diff(editor.head.line).to_string()
                        }
                        _ => (line + 1).to_string(),
                    };
                    let color = hsla(if line == editor.head.line {
                        editor.theme.text
                    } else {
                        editor.theme.text_muted
                    });
                    let runs = number_runs(label.len(), color);
                    let shaped = window.text_system().shape_line(
                        label.into(),
                        px(editor.font_size),
                        &runs,
                        None,
                    );
                    let x = bounds.left() + gutter_width - shaped.width;
                    (shaped, x)
                })
                .collect();
            Prepaint {
                layout: Layout {
                    bounds,
                    text_left,
                    line_height,
                    first,
                    lines,
                },
                gutter,
            }
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.read(cx);
        let focus = editor.focus.clone();
        let focused = focus.is_focused(window);
        let theme = editor.theme;
        let (scroll, head, selection) = (editor.scroll, editor.head, editor.selection());
        // Matches on the lines in view; `first_match` is the first one's index overall.
        let (matches, first_match, current) = match &editor.find {
            Some(find) => {
                let (first, shown) = (prepaint.layout.first, prepaint.layout.lines.len());
                let low = find
                    .matches
                    .partition_point(|range| range.start.line < first);
                let high = find
                    .matches
                    .partition_point(|range| range.start.line < first + shown);
                (find.matches[low..high].to_vec(), low, find.current)
            }
            None => (Vec::new(), 0, None),
        };
        let layout = &prepaint.layout;
        let (line_height, text_left) = (layout.line_height, layout.text_left);
        let top = |line: usize| bounds.top() + line_height * line as f32 - scroll.y;
        let left = layout.text_left - scroll.x;
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let text_area = Bounds::from_corners(
            point(layout.text_left - px(2.0), bounds.top()),
            bounds.bottom_right(),
        );
        // Partly shown lines at the edges stay inside the pane, off the status line.
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if selection.is_empty() {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.left(), top(head.line)),
                        size(bounds.size.width, line_height),
                    ),
                    hsla(theme.tab_active).opacity(0.6),
                ));
            }
            for (index, (shaped, x)) in prepaint.gutter.iter().enumerate() {
                let y = top(layout.first + index);
                if let Err(error) = shaped.paint(
                    point(*x, y),
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                ) {
                    eprintln!("bareframe: painting line numbers: {error}");
                }
            }
        });
        window.with_content_mask(Some(gpui::ContentMask { bounds: text_area }), |window| {
            for (index, range) in matches.iter().enumerate() {
                let Some(shaped) = range
                    .start
                    .line
                    .checked_sub(layout.first)
                    .and_then(|index| layout.lines.get(index))
                else {
                    continue;
                };
                let y = top(range.start.line);
                let strength = if Some(index + first_match) == current {
                    0.45
                } else {
                    0.18
                };
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(left + shaped.x_for_index(range.start.column), y),
                        point(left + shaped.x_for_index(range.end.column), y + line_height),
                    ),
                    hsla(theme.palette[3]).opacity(strength),
                ));
            }
            for (index, shaped) in layout.lines.iter().enumerate() {
                let line = layout.first + index;
                let y = top(line);
                if selection.start.line <= line
                    && line <= selection.end.line
                    && !selection.is_empty()
                {
                    let start = if line == selection.start.line {
                        shaped.x_for_index(selection.start.column)
                    } else {
                        Pixels::ZERO
                    };
                    let end = if line == selection.end.line {
                        shaped.x_for_index(selection.end.column)
                    } else {
                        // The line break is selected too.
                        shaped.width + px(6.0)
                    };
                    window.paint_quad(fill(
                        Bounds::from_corners(
                            point(left + start, y),
                            point(left + end, y + line_height),
                        ),
                        hsla(theme.accent).opacity(0.3),
                    ));
                }
                if let Err(error) = shaped.paint(
                    point(left, y),
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                ) {
                    eprintln!("bareframe: painting editor text: {error}");
                }
            }
            if focused
                && let Some(shaped) = head
                    .line
                    .checked_sub(layout.first)
                    .and_then(|index| layout.lines.get(index))
            {
                let x = left + shaped.x_for_index(head.column);
                window.paint_quad(fill(
                    Bounds::new(point(x, top(head.line)), size(px(2.0), line_height)),
                    hsla(theme.accent),
                ));
            }
        });
        let layout = std::mem::replace(
            &mut prepaint.layout,
            Layout {
                bounds,
                text_left,
                line_height,
                first: 0,
                lines: Vec::new(),
            },
        );
        self.editor
            .update(cx, |editor, _| editor.layout = Some(layout));
    }
}

impl Render for EditorPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let markdown = crate::markdown::is_markdown(&self.path);
        let editing = div()
            .flex_1()
            .min_h_0()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|e, _: &Left, _, cx| {
                let to = if e.anchor == e.head {
                    e.buffer.previous(e.head)
                } else {
                    e.selection().start
                };
                e.select(to, false, cx)
            }))
            .on_action(cx.listener(|e, _: &Right, _, cx| {
                let to = if e.anchor == e.head {
                    e.buffer.next(e.head)
                } else {
                    e.selection().end
                };
                e.select(to, false, cx)
            }))
            .on_action(
                cx.listener(|e, _: &SelectLeft, _, cx| {
                    e.select(e.buffer.previous(e.head), true, cx)
                }),
            )
            .on_action(
                cx.listener(|e, _: &SelectRight, _, cx| e.select(e.buffer.next(e.head), true, cx)),
            )
            .on_action(cx.listener(|e, _: &Up, window, cx| e.vertical(-1, false, window, cx)))
            .on_action(cx.listener(|e, _: &Down, window, cx| e.vertical(1, false, window, cx)))
            .on_action(cx.listener(|e, _: &SelectUp, window, cx| e.vertical(-1, true, window, cx)))
            .on_action(cx.listener(|e, _: &SelectDown, window, cx| e.vertical(1, true, window, cx)))
            .on_action(cx.listener(|e, _: &PageUp, window, cx| {
                let page = e.page();
                e.vertical(-page, false, window, cx)
            }))
            .on_action(cx.listener(|e, _: &PageDown, window, cx| {
                let page = e.page();
                e.vertical(page, false, window, cx)
            }))
            .on_action(cx.listener(|e, _: &WordLeft, _, cx| {
                e.select(e.buffer.word_start(e.head), false, cx)
            }))
            .on_action(
                cx.listener(|e, _: &WordRight, _, cx| {
                    e.select(e.buffer.word_end(e.head), false, cx)
                }),
            )
            .on_action(cx.listener(|e, _: &SelectWordLeft, _, cx| {
                e.select(e.buffer.word_start(e.head), true, cx)
            }))
            .on_action(cx.listener(|e, _: &SelectWordRight, _, cx| {
                e.select(e.buffer.word_end(e.head), true, cx)
            }))
            .on_action(cx.listener(|e, _: &LineStart, _, cx| {
                e.select(line_start(&e.buffer, e.head), false, cx)
            }))
            .on_action(cx.listener(|e, _: &LineEnd, _, cx| {
                let end = Position::new(e.head.line, e.buffer.line(e.head.line).len());
                e.select(end, false, cx)
            }))
            .on_action(cx.listener(|e, _: &SelectLineStart, _, cx| {
                e.select(line_start(&e.buffer, e.head), true, cx)
            }))
            .on_action(cx.listener(|e, _: &SelectLineEnd, _, cx| {
                let end = Position::new(e.head.line, e.buffer.line(e.head.line).len());
                e.select(end, true, cx)
            }))
            .on_action(
                cx.listener(|e, _: &DocumentStart, _, cx| e.select(Position::default(), false, cx)),
            )
            .on_action(cx.listener(|e, _: &DocumentEnd, _, cx| e.select(e.buffer.end(), false, cx)))
            .on_action(cx.listener(|e, _: &SelectDocumentStart, _, cx| {
                e.select(Position::default(), true, cx)
            }))
            .on_action(
                cx.listener(|e, _: &SelectDocumentEnd, _, cx| e.select(e.buffer.end(), true, cx)),
            )
            .on_action(cx.listener(|e, _: &Collapse, _, cx| e.select(e.head, false, cx)))
            .on_action(cx.listener(|e, _: &SelectAll, _, cx| {
                e.anchor = Position::default();
                e.select(e.buffer.end(), true, cx)
            }))
            .on_action(cx.listener(|e, _: &Backspace, _, cx| e.delete_with(Buffer::previous, cx)))
            .on_action(cx.listener(|e, _: &Delete, _, cx| e.delete_with(Buffer::next, cx)))
            .on_action(
                cx.listener(|e, _: &DeleteWordLeft, _, cx| e.delete_with(Buffer::word_start, cx)),
            )
            .on_action(cx.listener(|e, _: &DeleteLineLeft, _, cx| e.delete_with(line_start, cx)))
            .on_action(cx.listener(|e, _: &Newline, _, cx| {
                // The new line starts at the same indent as this one.
                let indent = e.buffer.indent(e.head.line).to_owned();
                e.insert(&format!("\n{indent}"), cx)
            }))
            .on_action(cx.listener(|e, _: &Tab, _, cx| {
                let unit = e.indent_unit();
                e.insert(unit, cx)
            }))
            .on_action(cx.listener(|e, _: &Copy, _, cx| {
                let text = copied(e);
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }))
            .on_action(cx.listener(|e, _: &Cut, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied(e)));
                if e.anchor == e.head {
                    // Like Copy, Cut without a selection takes the whole line.
                    let line = e.head.line;
                    let end = e
                        .buffer
                        .next(Position::new(line, e.buffer.line(line).len()));
                    e.replace(Position::new(line, 0)..end, "", cx);
                } else {
                    e.insert("", cx);
                }
            }))
            .on_action(cx.listener(|e, _: &Paste, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    e.insert(&text, cx)
                }
            }))
            .on_action(cx.listener(|e, _: &Undo, _, cx| {
                if !e.read_only
                    && let Some(range) = e.buffer.undo()
                {
                    e.highlight(cx);
                    e.anchor = range.start;
                    e.select(range.end, true, cx);
                    cx.emit(EditorEvent::Changed);
                }
            }))
            .on_action(cx.listener(|e, _: &Redo, _, cx| {
                if !e.read_only
                    && let Some(range) = e.buffer.redo()
                {
                    e.highlight(cx);
                    e.select(range.end, false, cx);
                    cx.emit(EditorEvent::Changed);
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|e, _, _, _| e.selecting = false),
            )
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(EditorElement {
                editor: cx.entity(),
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.terminal_background))
            // A Markdown file's raw view answers both the file's keys and Cmd+E.
            .key_context(if markdown {
                MARKDOWN_FILE_CONTEXT
            } else {
                FILE_CONTEXT
            })
            .when(markdown, |this| {
                this.on_action(
                    cx.listener(|e, _: &crate::markdown::ToggleRaw, _, cx| e.show_rendered(cx)),
                )
                .child(crate::markdown::view_switch(theme, true, {
                    let editor = cx.entity().downgrade();
                    move |_, cx| {
                        editor
                            .update(cx, |editor, cx| editor.show_rendered(cx))
                            .ok();
                    }
                }))
            })
            .on_action(cx.listener(|e, _: &Save, _, cx| {
                e.save(cx);
            }))
            .on_action(cx.listener(|e, _: &Find, window, cx| e.open_find(false, window, cx)))
            .on_action(cx.listener(|e, _: &FindReplace, window, cx| e.open_find(true, window, cx)))
            .on_action(cx.listener(|e, _: &FindNext, _, cx| e.find_step(true, cx)))
            .on_action(cx.listener(|e, _: &FindPrevious, _, cx| e.find_step(false, cx)))
            .on_action(cx.listener(|e, _: &GoToLine, window, cx| e.open_go_to(window, cx)))
            .children(self.render_notice(cx))
            .children(self.render_bars(cx))
            .child(editing)
            .child(self.render_status())
    }
}

/// Cmd+Left: to the first non-blank character, or to column 0 if already there.
fn line_start(buffer: &Buffer, position: Position) -> Position {
    let indent = buffer.indent(position.line).len();
    let column = if position.column == indent { 0 } else { indent };
    Position::new(position.line, column)
}

/// The selection, or the whole line (with its break) when nothing is selected.
fn copied(editor: &EditorPane) -> String {
    if editor.anchor != editor.head {
        return editor.buffer.slice(editor.selection());
    }
    format!("{}\n", editor.buffer.line(editor.head.line))
}

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let context = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("alt-left", WordLeft, context),
        KeyBinding::new("alt-right", WordRight, context),
        KeyBinding::new("alt-shift-left", SelectWordLeft, context),
        KeyBinding::new("alt-shift-right", SelectWordRight, context),
        KeyBinding::new("cmd-left", LineStart, context),
        KeyBinding::new("cmd-right", LineEnd, context),
        KeyBinding::new("home", LineStart, context),
        KeyBinding::new("end", LineEnd, context),
        KeyBinding::new("ctrl-a", LineStart, context),
        KeyBinding::new("ctrl-e", LineEnd, context),
        KeyBinding::new("cmd-shift-left", SelectLineStart, context),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, context),
        KeyBinding::new("shift-home", SelectLineStart, context),
        KeyBinding::new("shift-end", SelectLineEnd, context),
        KeyBinding::new("cmd-up", DocumentStart, context),
        KeyBinding::new("cmd-down", DocumentEnd, context),
        KeyBinding::new("cmd-shift-up", SelectDocumentStart, context),
        KeyBinding::new("cmd-shift-down", SelectDocumentEnd, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("shift-backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("alt-backspace", DeleteWordLeft, context),
        KeyBinding::new("cmd-backspace", DeleteLineLeft, context),
        KeyBinding::new("enter", Newline, context),
        KeyBinding::new("tab", Tab, context),
        KeyBinding::new("escape", Collapse, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
        KeyBinding::new("cmd-s", Save, Some(FILE_CONTEXT)),
        KeyBinding::new("cmd-f", Find, Some(FILE_CONTEXT)),
        KeyBinding::new("cmd-alt-f", FindReplace, Some(FILE_CONTEXT)),
        KeyBinding::new("cmd-g", FindNext, Some(FILE_CONTEXT)),
        KeyBinding::new("cmd-shift-g", FindPrevious, Some(FILE_CONTEXT)),
        KeyBinding::new("ctrl-g", GoToLine, Some(FILE_CONTEXT)),
    ]
}

/// The terminal palette's colors, so code looks like the rest of the theme.
fn syntax_color(theme: &Theme, syntax: Syntax) -> u32 {
    match syntax {
        Syntax::Keyword => theme.palette[5],
        Syntax::String => theme.palette[2],
        Syntax::Comment => theme.text_muted,
        Syntax::Number => theme.palette[3],
        Syntax::Function => theme.palette[4],
        Syntax::Type => theme.palette[6],
        Syntax::Tag | Syntax::Builtin => theme.palette[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_are_told_from_binaries() {
        let dir = std::env::temp_dir().join(format!("bareframe-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = dir.join("a.rs");
        std::fs::write(&text, "fn main() {}\n// 한글\n").unwrap();
        let binary = dir.join("a.bin");
        std::fs::write(&binary, [0u8, 1, 2, 3]).unwrap();
        // A multi-byte character cut by the 8 KB read is still text.
        let cut = dir.join("cut.txt");
        let mut long = "a".repeat(8191);
        long.push('한');
        std::fs::write(&cut, long).unwrap();
        assert!(is_text_file(&text));
        assert!(!is_text_file(&binary));
        assert!(is_text_file(&cut));
        assert!(!is_text_file(&dir), "a directory is not a text file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn line_start_toggles_between_indent_and_column_zero() {
        let buffer = Buffer::new("    let x = 1;");
        assert_eq!(
            line_start(&buffer, Position::new(0, 10)),
            Position::new(0, 4)
        );
        assert_eq!(
            line_start(&buffer, Position::new(0, 4)),
            Position::new(0, 0)
        );
    }
}
