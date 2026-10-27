//! Laying out and painting the text: syntax colors, the gutter, the selection, find
//! matches, the cursor and the text being composed.
use super::{EditorPane, LINE_HEIGHT, LineNumbers, MAX_LINE};
use crate::{
    syntax::{self, Syntax},
    theme::{Theme, hsla},
};
use bareframe_core::text::Position;
use gpui::{
    App, Bounds, Context, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId,
    Pixels, ShapedLine, Style, TextRun, UnderlineStyle, Window, fill, point, prelude::*, px,
    relative, size,
};
use std::{ops::Range, time::Duration};

/// What the last paint laid out, for clicks, input method boxes and Up/Down.
pub(super) struct Layout {
    pub(super) bounds: Bounds<Pixels>,
    /// Where column 0 is drawn when not scrolled sideways.
    pub(super) text_left: Pixels,
    pub(super) line_height: Pixels,
    pub(super) first: usize,
    pub(super) lines: Vec<ShapedLine>,
}

impl EditorPane {
    pub(super) fn line_height(&self) -> Pixels {
        px((self.font_size * LINE_HEIGHT).round())
    }

    pub(super) fn base_font(&self) -> gpui::Font {
        gpui::font(self.font.clone())
    }

    pub(super) fn shape(&self, line: usize, window: &mut Window) -> ShapedLine {
        let full = self.buffer.line(line);
        let mut cut = full.len().min(MAX_LINE);
        while !full.is_char_boundary(cut) {
            cut -= 1;
        }
        let text = full[..cut].to_owned();
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
    pub(super) fn highlight(&mut self, cx: &mut Context<Self>) {
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

    /// Scrolls so the cursor is in view, after a move or an edit.
    fn reveal_cursor(
        &mut self,
        bounds: Bounds<Pixels>,
        text_left: Pixels,
        em: Pixels,
        window: &mut Window,
    ) {
        let line_height = self.line_height();
        let top = line_height * self.head.line as f32;
        if top < self.scroll.y {
            self.scroll.y = top;
        } else if top + line_height > self.scroll.y + bounds.size.height {
            self.scroll.y = top + line_height - bounds.size.height;
        }
        let x = self.x_of(self.head, window);
        let room = bounds.right() - text_left - em * 2.0;
        if x < self.scroll.x {
            self.scroll.x = (x - em * 4.0).max(Pixels::ZERO);
        } else if x > self.scroll.x + room {
            self.scroll.x = x - room;
        }
    }

    /// The numbers of `lines`, each with the x that right-aligns it to `right`.
    fn shape_gutter(
        &self,
        lines: Range<usize>,
        right: Pixels,
        window: &mut Window,
    ) -> Vec<(ShapedLine, Pixels)> {
        if self.line_numbers == LineNumbers::Off {
            return Vec::new();
        }
        lines
            .map(|line| {
                // Relative numbers count from the cursor's line, which shows its own.
                let label = match self.line_numbers {
                    LineNumbers::Relative if line != self.head.line => {
                        line.abs_diff(self.head.line).to_string()
                    }
                    _ => (line + 1).to_string(),
                };
                let color = hsla(if line == self.head.line {
                    self.theme.text
                } else {
                    self.theme.text_muted
                });
                let runs = [TextRun {
                    len: label.len(),
                    font: self.base_font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }];
                let shaped =
                    window
                        .text_system()
                        .shape_line(label.into(), px(self.font_size), &runs, None);
                let x = right - shaped.width;
                (shaped, x)
            })
            .collect()
    }
}

pub(super) struct EditorElement {
    pub(super) editor: Entity<EditorPane>,
}

pub(super) struct Prepaint {
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
            if std::mem::take(&mut editor.reveal) {
                editor.reveal_cursor(bounds, text_left, em, window);
            }
            let first = (editor.scroll.y / line_height).floor().max(0.0) as usize;
            let count = (bounds.size.height / line_height).ceil() as usize + 1;
            let last = (first + count).min(editor.buffer.line_count());
            let lines = (first..last)
                .map(|line| editor.shape(line, window))
                .collect();
            let gutter = editor.shape_gutter(first..last, bounds.left() + gutter_width, window);
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
        let (first, shown) = (prepaint.layout.first, prepaint.layout.lines.len());
        let (matches, first_match, current) = editor.matches_in(first..first + shown);
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
                if let Some((start, end)) = selected_span(&selection, line, shaped) {
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

/// Where the selection covers `line`, as x offsets into the shaped line.
fn selected_span(
    selection: &Range<Position>,
    line: usize,
    shaped: &ShapedLine,
) -> Option<(Pixels, Pixels)> {
    if selection.is_empty() || line < selection.start.line || selection.end.line < line {
        return None;
    }
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
    Some((start, end))
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
