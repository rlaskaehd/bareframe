//! The side columns around the panes (the tab sidebar, the tools panel and the Code review
//! panel): their widths, the 1px lines on their edges and resizing them by those edges.
use super::WorkspaceView;
use crate::theme::{Theme, hsla};
use gpui::{Context, CursorStyle, MouseButton, MouseMoveEvent, Window, div, prelude::*, px};

pub(super) const SIDEBAR_DEFAULT_WIDTH: f32 = 248.0;
pub(super) const TOOLS_DEFAULT_WIDTH: f32 = 260.0;
pub(super) const REVIEW_DEFAULT_WIDTH: f32 = 520.0;

#[derive(Clone, Copy)]
pub(super) enum Column {
    Sidebar,
    Tools,
    Review,
}

impl Column {
    /// The widest share of the window and the narrowest width the column may take.
    fn limits(self) -> (f32, f32) {
        match self {
            // As Warp's vertical tabs panel.
            Self::Sidebar => (0.5, 200.0),
            Self::Tools => (0.4, 180.0),
            Self::Review => (0.6, 300.0),
        }
    }

    fn clamp(self, width: f32, window: &Window) -> f32 {
        let (share, min) = self.limits();
        width
            .min(f32::from(window.viewport_size().width) * share)
            .max(min)
    }
}

impl WorkspaceView {
    /// Re-clamped on every render, so shrinking the window can't squeeze the panes behind
    /// a column sized for a wider one.
    pub(super) fn column_width(&self, column: Column, window: &Window) -> f32 {
        let width = match column {
            Column::Sidebar => self.sidebar_width,
            Column::Tools => self.tools_width,
            Column::Review => self.review_width,
        };
        column.clamp(width, window)
    }

    /// The border on the right of a column left of the panes, grabbed for resizing it.
    pub(super) fn column_edge(&self, column: Column, cx: &mut Context<Self>) -> gpui::Div {
        edge_line(self.theme()).child(self.resize_grip(column, cx).right_0())
    }

    /// The 5px grab area for resizing `column`, laid over the column's own edge. Its parent
    /// places it with `left_0`/`right_0`; it must be painted after the column to catch the
    /// mouse.
    pub(super) fn resize_grip(&self, column: Column, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .absolute()
            .top_0()
            .h_full()
            .w(px(5.0))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.resizing_column = Some(column);
                }),
            )
    }

    /// Follows a column edge being dragged; false when none is.
    pub(super) fn drag_column_edge(&mut self, event: &MouseMoveEvent, window: &Window) -> bool {
        let Some(column) = self.resizing_column else {
            return false;
        };
        if !event.dragging() {
            self.resizing_column = None;
            return true;
        }
        let x = f32::from(event.position.x);
        let (width, wanted) = match column {
            Column::Sidebar => (&mut self.sidebar_width, x),
            Column::Tools => (&mut self.tools_width, x - self.tools_left),
            Column::Review => (
                &mut self.review_width,
                f32::from(window.viewport_size().width) - x,
            ),
        };
        *width = column.clamp(wanted, window);
        true
    }
}

/// A side column's border: one line that the pane header lines meet with no gap.
pub(super) fn edge_line(theme: &Theme) -> gpui::Div {
    div()
        .relative()
        .flex_none()
        .w(px(1.0))
        .h_full()
        .bg(hsla(theme.border))
}
