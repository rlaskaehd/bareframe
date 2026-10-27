//! Selecting the rendered text with the mouse and copying it. Every stretch of text a block
//! draws is a piece, numbered in reading order (the ids `render_block` hands out), and a
//! selection runs from a place in one piece to a place in another.
use super::MarkdownPane;
use gpui::{
    ClipboardItem, Context, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, StyledText,
    TextLayout, TextRun, div, prelude::*,
};
use std::ops::Range;

/// A place in the rendered text: a piece and a byte offset in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Place {
    piece: usize,
    offset: usize,
}

pub(super) struct Selection {
    anchor: Place,
    head: Place,
    /// The button is still down.
    dragging: bool,
}

impl Selection {
    fn ordered(&self) -> (Place, Place) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }
}

impl MarkdownPane {
    /// The part of piece `id` (of `len` bytes) that is selected.
    fn selected_in(&self, id: usize, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.selection.as_ref()?.ordered();
        if id < start.piece || id > end.piece || start == end {
            return None;
        }
        let from = if id == start.piece { start.offset } else { 0 };
        let to = if id == end.piece { end.offset } else { len };
        (from < to).then_some(from..to.min(len))
    }

    /// `text` drawn with `runs` as piece `id`: remembered for copying, its selected part
    /// shaded, and the mouse on it starts or extends a selection. `wrap` turns the styled
    /// text into the element to show, so links stay clickable.
    pub(super) fn selectable(
        &self,
        id: usize,
        text: String,
        runs: Vec<TextRun>,
        shade: Hsla,
        wrap: impl FnOnce(StyledText) -> gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let runs = match self.selected_in(id, text.len()) {
            Some(range) => shade_runs(runs, range, shade),
            None => runs,
        };
        self.pieces.borrow_mut().insert(id, text.clone());
        let styled = StyledText::new(text).with_runs(runs);
        let layout = styled.layout().clone();
        let moved = layout.clone();
        div()
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |pane, event: &MouseDownEvent, window, cx| {
                    pane.focus.focus(window, cx);
                    let at = place(&layout, id, event.position);
                    let (anchor, head) = if event.modifiers.shift
                        && let Some(selection) = &pane.selection
                    {
                        (selection.anchor, at)
                    } else {
                        (at, at)
                    };
                    pane.selection = Some(Selection {
                        anchor,
                        head,
                        dragging: true,
                    });
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |pane, event: &MouseMoveEvent, _, cx| {
                let Some(selection) = pane.selection.as_mut().filter(|s| s.dragging) else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    selection.dragging = false;
                    return;
                }
                let head = place(&moved, id, event.position);
                if selection.head != head {
                    selection.head = head;
                    cx.notify();
                }
            }))
            .child(wrap(styled))
            .into_any_element()
    }

    /// The button came up anywhere in the pane.
    pub(super) fn end_selection(&mut self) {
        if let Some(selection) = self.selection.as_mut() {
            selection.dragging = false;
        }
    }

    /// Cmd+C: the selected text, its pieces one per line.
    pub(super) fn copy_selection(&self, cx: &mut Context<Self>) {
        let Some(selection) = &self.selection else {
            return;
        };
        let (start, end) = selection.ordered();
        let pieces = self.pieces.borrow();
        let text: Vec<&str> = pieces
            .range(start.piece..=end.piece)
            .filter_map(|(&id, text)| text.get(self.selected_in(id, text.len())?))
            .collect();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.join("\n")));
        }
    }
}

fn place(layout: &TextLayout, piece: usize, at: gpui::Point<gpui::Pixels>) -> Place {
    let offset = layout
        .index_for_position(at)
        .unwrap_or_else(|nearest| nearest);
    Place { piece, offset }
}

/// `runs` with `range` given a background, split where the range starts and ends.
fn shade_runs(runs: Vec<TextRun>, range: Range<usize>, shade: Hsla) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut at = 0;
    for run in runs {
        let end = at + run.len;
        let cuts = [
            at,
            range.start.clamp(at, end),
            range.end.clamp(at, end),
            end,
        ];
        for pair in cuts.windows(2) {
            if pair[1] > pair[0] {
                let inside = range.start <= pair[0] && pair[1] <= range.end;
                out.push(TextRun {
                    len: pair[1] - pair[0],
                    background_color: if inside {
                        Some(shade)
                    } else {
                        run.background_color
                    },
                    ..run.clone()
                });
            }
        }
        at = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(len: usize) -> TextRun {
        TextRun {
            len,
            font: gpui::font("x"),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn shading_splits_runs_at_the_selection() {
        let shade = gpui::red();
        let runs = shade_runs(vec![run(4), run(6)], 2..7, shade);
        let spans: Vec<(usize, bool)> = runs
            .iter()
            .map(|run| (run.len, run.background_color == Some(shade)))
            .collect();
        assert_eq!(spans, [(2, false), (2, true), (3, true), (3, false)]);
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), 10);
    }
}
