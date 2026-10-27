//! The single-line text field (renaming tabs and groups, search, find, the commit message),
//! adapted from GPUI's input example. It is an IME client like the terminal, so Korean
//! composition works while typing.
use bareframe_core::text::{byte_column, utf16_column};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, GlobalElementId, LayoutId, MouseButton,
    MouseDownEvent, PaintQuad, Pixels, ShapedLine, Style, Subscription, TextRun, UTF16Selection,
    UnderlineStyle, Window, actions, div, fill, point, prelude::*, px, relative, size,
};
use std::ops::Range;

actions!(
    text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Confirm,
        Cancel,
        Copy,
        Paste,
        Cut,
    ]
);

/// Keys only apply while a text field has focus, so the terminal keeps its own.
const KEY_CONTEXT: &str = "TextField";

pub enum TextFieldEvent {
    Confirm(String),
    Cancel,
}

pub struct TextField {
    focus: FocusHandle,
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// How far the text is shifted left so the cursor stays inside the field.
    scroll: Pixels,
    accent: u32,
    finished: bool,
    /// Rename fields commit on blur and finish once; the search field never finishes.
    blur: Option<Subscription>,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl TextField {
    /// Starts focused with `text` selected, so typing replaces it.
    pub fn new(text: &str, accent: u32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = Self::search(accent, window, cx);
        input.content = text.to_owned();
        input.selected_range = 0..text.len();
        // Clicking elsewhere commits, as in Warp.
        input.blur = Some(cx.on_blur(&input.focus, window, |input: &mut Self, _, cx| {
            input.finish(true, cx)
        }));
        input
    }

    /// An empty, focused field that reports Enter and Esc every time.
    pub fn search(accent: u32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            focus,
            selected_range: 0..0,
            content: String::new(),
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll: Pixels::ZERO,
            accent,
            finished: false,
            blur: None,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces the text and selects it all, so typing starts over.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_owned();
        self.selected_range = 0..text.len();
        self.marked_range = None;
        cx.notify();
    }

    fn finish(&mut self, confirm: bool, cx: &mut Context<Self>) {
        if self.blur.is_some() && std::mem::replace(&mut self.finished, true) {
            return;
        }
        cx.emit(if confirm {
            TextFieldEvent::Confirm(self.content.clone())
        } else {
            TextFieldEvent::Cancel
        });
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    // Character boundaries stand in for grapheme clusters: a Hangul syllable is one char.
    fn previous_boundary(&self, offset: usize) -> usize {
        self.content[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content[offset..]
            .chars()
            .next()
            .map_or(offset, |ch| offset + ch.len_utf8())
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        utf16_column(&self.content, range.start)..utf16_column(&self.content, range.end)
    }

    /// What the input method replaces: the range it names, else the text being composed,
    /// else the selection.
    fn input_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone())
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        byte_column(&self.content, range.start)..byte_column(&self.content, range.end)
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };
        self.move_to(offset, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.selected_range.is_empty() {
            self.next_boundary(self.selected_range.end)
        } else {
            self.selected_range.end
        };
        self.move_to(offset, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let line: String = text
                .chars()
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect();
            self.replace_text_in_range(None, &line, window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus.focus(window, cx);
        let (Some(bounds), Some(line)) = (self.last_bounds, self.last_layout.as_ref()) else {
            return;
        };
        let offset = line.closest_index_for_x(event.position.x - bounds.left() + self.scroll);
        self.move_to(offset, cx);
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.input_range(range_utf16);
        self.content.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.input_range(range_utf16);
        self.content.replace_range(range.clone(), new_text);
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selected| self.range_from_utf16(selected))
            .map(|selected| range.start + selected.start..range.start + selected.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(range.start) - self.scroll,
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(range.end) - self.scroll,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let index = self
            .last_layout
            .as_ref()?
            .index_for_x(point.x - bounds.left() + self.scroll)?;
        Some(utf16_column(&self.content, index))
    }
}

struct TextElement {
    input: Entity<TextField>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    scroll: Pixels,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
        style.size.height = window.line_height().into();
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
        let input = self.input.read(cx);
        let style = window.text_style();
        let run = TextRun {
            len: input.content.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match input.marked_range.clone() {
            Some(marked) => vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: input.content.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line =
            window
                .text_system()
                .shape_line(input.content.clone().into(), font_size, &runs, None);
        let accent = crate::theme::hsla(input.accent);
        let selected = input.selected_range.clone();
        // Keep the cursor visible: shift the text left once it passes the right edge.
        let cursor_x = line.x_for_index(input.cursor_offset());
        let scroll = (cursor_x - bounds.size.width + px(2.0)).max(Pixels::ZERO);
        let left = bounds.left() - scroll;
        let (selection, cursor) = if selected.is_empty() {
            let x = cursor_x;
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(left + x, bounds.top()),
                        size(px(1.5), bounds.size.height),
                    ),
                    accent,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(selected.start), bounds.top()),
                        point(left + line.x_for_index(selected.end), bounds.bottom()),
                    ),
                    accent.opacity(0.35),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            scroll,
            cursor,
            selection,
        }
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
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let scroll = prepaint.scroll;
        let Some(line) = prepaint.line.take() else {
            return;
        };
        // Long names would otherwise paint over the row's buttons.
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            if let Err(error) = line.paint(
                point(bounds.left() - scroll, bounds.top()),
                window.line_height(),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            ) {
                eprintln!("bareframe: painting rename field: {error}");
            }
            if focus.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll = scroll;
        });
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|input, _: &Confirm, _, cx| input.finish(true, cx)))
            .on_action(cx.listener(|input, _: &Cancel, _, cx| input.finish(false, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .child(TextElement { input: cx.entity() })
    }
}

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let context = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Cancel, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
    ]
}
