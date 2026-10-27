//! Warp's code editor: a text file in a pane with a cursor and selection, the usual
//! movement and editing keys, undo and the clipboard. Drawing, find, the file on disk and
//! the input method each have a module below.
use crate::{
    syntax::{self, LineSyntax},
    text_field::TextField,
    theme::{Theme, hsla},
    watch::Watch,
};
use bareframe_core::text::{Buffer, Position};
use gpui::{
    ClipboardItem, Context, CursorStyle, Entity, EventEmitter, FocusHandle, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollWheelEvent, Subscription, Task, Window,
    actions, div, prelude::*,
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod element;
mod file;
mod find;
mod input;
mod lsp;
mod status;

use element::{EditorElement, Layout};
use file::Notice;
pub use file::is_text_file;
use find::FindBar;

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
/// Lines longer than this (minified files) make the file read-only and are drawn only up
/// to here; laying out megabytes for one line each frame would stall.
const MAX_LINE: usize = 64 << 10;
const LINE_HEIGHT: f32 = 1.45;

pub enum EditorEvent {
    /// The text or its saved state changed; titles show a dot for unsaved changes.
    Changed,
    /// The user chose Save or Don't Save when asked; the pane may go.
    Close,
    /// A Markdown file's `Rendered` view was asked for, and nothing unsaved is in the way.
    ShowRendered,
    /// Cancel was chosen when asked about unsaved changes: whatever asked stops.
    Kept,
    /// Go to a line (1-based) of another file: a definition the language server found.
    OpenAt(PathBuf, u64),
    /// Run a command in a terminal at a directory: installing a language server.
    RunInTerminal(String, PathBuf),
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
    /// A hash of the file as last read or written, so our own save isn't taken for an
    /// outside edit; a hash rather than a second copy of the file.
    disk: u64,
    /// "Don't Save" was chosen: closing no longer asks.
    discarded: bool,
    /// The question about unsaved changes was asked to switch to `Rendered`, not to close.
    to_rendered: bool,
    layout: Option<Layout>,
    /// The grammar for this file's colors, if one is bundled.
    language: Option<&'static str>,
    find: Option<FindBar>,
    go_to: Option<(Entity<TextField>, Subscription)>,
    line_numbers: LineNumbers,
    autosave: bool,
    _autosave: Option<Task<()>>,
    /// Colors per line from the last parse; lines edited since have none until the next.
    syntax: Vec<LineSyntax>,
    _highlight: Option<Task<()>>,
    language_server: Option<lsp::Language>,
    _watch: Option<Watch>,
}

impl EventEmitter<EditorEvent> for EditorPane {}

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
            disk: fingerprint(&text),
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
            language_server: None,
            _watch: None,
        };
        if editor.buffer.longest_line() > MAX_LINE {
            editor.read_only = true;
        }
        editor.language = syntax::language_for(&editor.path);
        editor._watch = editor.watch(cx);
        editor.highlight(cx);
        editor.attach_language(cx);
        editor
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> String {
        let name = crate::explorer::file_name(&self.path);
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

    /// After any edit, undo or reload: colors and the language server catch up.
    fn text_changed(&mut self, cx: &mut Context<Self>) {
        self.highlight(cx);
        self.sync_language(cx);
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
        let old_end = self.buffer.line(first).len();
        let end = self.buffer.replace(range, text, Instant::now());
        // Until the next parse (a moment later) the edited line keeps its old colors, a
        // stretch that reached its end following it to the new end, so a heading being
        // typed doesn't blink plain. Lines the edit added have none yet.
        if last < self.syntax.len() {
            let new_end = self.buffer.line(first).len();
            let mut kept = std::mem::take(&mut self.syntax[first]);
            for (range, _) in &mut kept {
                if range.end == old_end {
                    range.end = new_end;
                }
            }
            kept.retain(|(range, _)| range.start < range.end);
            let added = std::iter::repeat_n(Vec::new(), end.line - first);
            self.syntax
                .splice(first..=last, std::iter::once(kept).chain(added));
        }
        self.text_changed(cx);
        if self.autosave {
            self._autosave = Some(cx.spawn(async move |editor, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                editor
                    .update(cx, |editor, cx| {
                        // Never over a change made outside while that question is open.
                        let asking = matches!(editor.notice, Some(Notice::ChangedOnDisk));
                        if editor.buffer.is_dirty() && !asking {
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
        if event.modifiers.platform && event.click_count == 1 {
            return self.go_to_definition(position, cx);
        }
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
            if event.pressed_button.is_none() {
                self.hover_at(event.position, cx);
            }
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
        self.hide_hover(cx);
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
        self.scroll.x = self.scroll.x.min(max_x);
    }

    fn indent_unit(&self) -> &'static str {
        let tabs = self
            .buffer
            .lines()
            .take(500)
            .any(|line| line.starts_with('\t'));
        if tabs { "\t" } else { "    " }
    }
}

impl EditorPane {
    /// The text itself, with the editing keys, mouse and scrolling.
    fn render_text(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("editor-text")
            .flex_1()
            .min_h_0()
            .on_hover(cx.listener(|e, hovered: &bool, _, cx| {
                if !hovered {
                    e.hide_hover(cx)
                }
            }))
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
                    e.text_changed(cx);
                    e.anchor = range.start;
                    e.select(range.end, true, cx);
                    cx.emit(EditorEvent::Changed);
                }
            }))
            .on_action(cx.listener(|e, _: &Redo, _, cx| {
                if !e.read_only
                    && let Some(range) = e.buffer.redo()
                {
                    e.text_changed(cx);
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
            })
    }
}

impl Render for EditorPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let markdown = crate::markdown::is_markdown(&self.path);
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
            .child(self.render_text(cx))
            .child(self.render_status(cx))
            .children(self.render_hover())
            .children(self.render_popup(cx))
    }
}

/// Cmd+Left: to the first non-blank character, or to column 0 if already there.
/// A strip across the top of the editor, as the find bar and file notices use.
fn bar(theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(hsla(theme.border))
        .bg(hsla(theme.tab_active))
        .font_family(crate::theme::UI_FONT)
        .text_size(gpui::px(12.0))
}

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
        // Cmd+G stays the tab group key; in the find field Enter goes on, Shift+Enter back.
        KeyBinding::new("shift-enter", FindPrevious, Some(FILE_CONTEXT)),
        KeyBinding::new("shift-enter", Newline, context),
        KeyBinding::new("ctrl-g", GoToLine, Some(FILE_CONTEXT)),
    ]
}

/// A file's contents, hashed to tell an outside edit from our own save.
fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

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
