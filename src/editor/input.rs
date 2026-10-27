//! The editor as the system input method's client, so Korean composition works.
use super::EditorPane;
use bareframe_core::text::Position;
use gpui::{
    Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, point, px, size,
};
use std::ops::Range;

impl EditorPane {
    /// What the input method replaces: the range it names, else the text being composed,
    /// else the selection.
    fn input_range(&self, range_utf16: Option<Range<usize>>) -> Range<Position> {
        match range_utf16 {
            Some(range) => self.buffer.from_utf16(range.start)..self.buffer.from_utf16(range.end),
            None => self.marked.clone().unwrap_or_else(|| self.selection()),
        }
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
        let range = self.input_range(range_utf16);
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
        let range = self.input_range(range_utf16);
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
