//! The active tab's split layout, the pane headers and dragging panes (or sidebar tabs)
//! beside other panes, as in Warp.
use super::{PaneView, Resize, WorkspaceView, icon_button, sidebar::DraggedTab};
use crate::theme::{self, Theme, hsla};
use bareframe_core::workspace::{Axis, Direction, Layout, PaneId, TabId};
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, DragMoveEvent, MouseButton, Pixels, Point, Window,
    canvas, div, prelude::*, px, relative,
};

/// A pane lifted by its header: a small card with its title follows the cursor.
#[derive(Clone)]
pub(super) struct DraggedPane {
    pub(super) pane: PaneId,
    title: String,
    theme: &'static Theme,
}

impl Render for DraggedPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .px(px(12.0))
            .py(px(6.0))
            .rounded_md()
            .bg(hsla(theme.tab_active))
            .border_1()
            .border_color(hsla(theme.accent).opacity(0.6))
            .shadow_lg()
            .opacity(0.92)
            .font_family(theme::UI_FONT)
            .text_size(px(12.0))
            .text_color(hsla(theme.text))
            .child(self.title.clone())
    }
}

/// The side of `bounds` whose edge is nearest `position`.
fn nearest_side(bounds: Bounds<Pixels>, position: Point<Pixels>) -> Direction {
    let x = f32::from(position.x - bounds.left()) / f32::from(bounds.size.width);
    let y = f32::from(position.y - bounds.top()) / f32::from(bounds.size.height);
    [
        (x, Direction::Left),
        (1.0 - x, Direction::Right),
        (y, Direction::Up),
        (1.0 - y, Direction::Down),
    ]
    .into_iter()
    .min_by(|a, b| a.0.total_cmp(&b.0))
    .map_or(Direction::Right, |(_, side)| side)
}

/// The half of a pane a drop would give the dragged pane or tab.
fn drop_zone(side: Direction, theme: &Theme) -> impl IntoElement {
    let zone = div()
        .absolute()
        .rounded_sm()
        .border_2()
        .border_color(hsla(theme.accent).opacity(0.7))
        .bg(hsla(theme.accent).opacity(0.18));
    match side {
        Direction::Left => zone.top_0().bottom_0().left_0().w(relative(0.5)),
        Direction::Right => zone.top_0().bottom_0().right_0().w(relative(0.5)),
        Direction::Up => zone.left_0().right_0().top_0().h(relative(0.5)),
        Direction::Down => zone.left_0().right_0().bottom_0().h(relative(0.5)),
    }
}

impl WorkspaceView {
    /// A pane or tab drag begins: the terminals freeze so drop zones can be drawn over them.
    pub(super) fn start_drag(&mut self, cx: &mut Context<Self>) {
        if !self.frozen {
            self.set_frozen(true, cx);
            self.drag_frozen = true;
        }
        cx.notify();
    }

    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.drop_target = None;
        if std::mem::take(&mut self.drag_frozen) {
            self.set_frozen(false, cx);
        }
        cx.notify();
    }

    /// Follows the drag over pane `id`: a drop there lands on the side nearest the cursor.
    fn hover_drop(
        &mut self,
        id: PaneId,
        allowed: bool,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let target =
            (allowed && bounds.contains(&position)).then(|| (id, nearest_side(bounds, position)));
        if target.is_some() && self.drop_target != target
            || target.is_none() && self.drop_target.is_some_and(|(pane, _)| pane == id)
        {
            self.drop_target = target;
            cx.notify();
        }
    }

    /// The side of pane `id` a drop lands on, ending the drag either way.
    fn take_drop(&mut self, id: PaneId, cx: &mut Context<Self>) -> Option<Direction> {
        let side = self
            .drop_target
            .filter(|(pane, _)| *pane == id)
            .map(|(_, side)| side);
        self.dragging.clear();
        self.end_drag(cx);
        side
    }

    fn drop_pane(&mut self, id: PaneId, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(side) = self.take_drop(id, cx) {
            self.model.move_pane(pane, id, side);
        }
        self.sync(window, cx);
    }

    fn drop_tabs(
        &mut self,
        id: PaneId,
        tabs: &[TabId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(side) = self.take_drop(id, cx) {
            self.model.merge_tabs(tabs, id, side);
        }
        self.sync(window, cx);
    }

    /// Shown once a tab is split, as in Warp: the title, a menu and a close button, with a
    /// corner mark on the focused pane.
    fn render_pane_header(&self, id: PaneId, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let active = id == self.model.active_pane();
        let title = self.pane_title(id, cx);
        let key = id.0 as usize;
        let dragged = DraggedPane {
            pane: id,
            title: title.clone(),
            theme,
        };
        let weak = cx.entity().downgrade();
        div()
            .id(("pane-header", key))
            .on_drag(dragged, move |dragged, _, _, cx| {
                if let Err(error) = weak.update(cx, |view, cx| view.start_drag(cx)) {
                    eprintln!("bareframe: starting a pane drag: {error}");
                }
                cx.new(|_| dragged.clone())
            })
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .h(px(28.0))
            .px(px(56.0))
            .border_b_1()
            .border_color(hsla(theme.border))
            .font_family(theme::UI_FONT)
            .text_size(px(12.0))
            .text_color(hsla(if active { theme.text } else { theme.text_muted }))
            .when(active, |this| {
                this.child(
                    gpui::svg()
                        .path("icons/corner.svg")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(10.0))
                        .text_color(hsla(theme.accent)),
                )
            })
            .child(div().min_w_0().truncate().child(title))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right(px(6.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        icon_button(("pane-menu", key), "⋮", theme).on_click(cx.listener(
                            move |view, event: &gpui::ClickEvent, window, cx| {
                                cx.stop_propagation();
                                view.open_pane_menu(id, event.position(), window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(("pane-close", key), "×", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_pane(id, window, cx)
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_layout(
        &self,
        layout: &Layout,
        path: &mut Vec<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme();
        match layout {
            Layout::Pane(id) => {
                let id = *id;
                let split = self.model.active().layout.panes().len() > 1;
                let pane = self.pane(id).map(PaneView::view);
                let terminal = self.terminal(id).is_some();
                let zone = self
                    .drop_target
                    .filter(|(pane, _)| *pane == id)
                    .map(|(_, side)| side);
                div()
                    .relative()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .on_drag_move(cx.listener(
                        move |view, event: &DragMoveEvent<DraggedPane>, _, cx| {
                            let allowed = event.drag(cx).pane != id;
                            view.hover_drop(id, allowed, event.bounds, event.event.position, cx)
                        },
                    ))
                    .on_drag_move(cx.listener(
                        move |view, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            let own = view.model.active().id;
                            let allowed = !event.drag(cx).tabs.contains(&own);
                            view.hover_drop(id, allowed, event.bounds, event.event.position, cx)
                        },
                    ))
                    .on_drop(cx.listener(move |view, dragged: &DraggedPane, window, cx| {
                        view.drop_pane(id, dragged.pane, window, cx)
                    }))
                    .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                        view.drop_tabs(id, &dragged.tabs, window, cx)
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, window, cx| {
                            if view.model.active_pane() != id && view.model.focus(id) {
                                view.sync(window, cx);
                            }
                        }),
                    )
                    .when(split, |this| this.child(self.render_pane_header(id, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            // Breathing room around the text; the surface can't be padded from GPUI.
                            .when(terminal, |this| this.p(px(6.0)))
                            .children(pane),
                    )
                    .children(zone.map(|side| drop_zone(side, theme)))
                    .into_any_element()
            }
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = *axis;
                let here = path.clone();
                path.push(false);
                let first = self.render_layout(first, path, cx);
                path.pop();
                path.push(true);
                let second = self.render_layout(second, path, cx);
                path.pop();
                let weak = cx.entity().downgrade();
                let recorded = here.clone();
                let tab = self.model.active_tab;
                let divider = div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .items_center()
                    .bg(hsla(theme.terminal_background))
                    .map(|this| match axis {
                        Axis::Horizontal => this
                            .w(px(5.0))
                            .h_full()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .child(div().w(px(1.0)).h_full().bg(hsla(theme.border))),
                        Axis::Vertical => this
                            .h(px(5.0))
                            .w_full()
                            .cursor(CursorStyle::ResizeUpDown)
                            .child(div().h(px(1.0)).w_full().bg(hsla(theme.border))),
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.resize = Some(Resize {
                                tab,
                                path: here.clone(),
                                axis,
                            });
                        }),
                    );
                div()
                    .relative()
                    .flex()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .map(|this| match axis {
                        Axis::Horizontal => this.flex_row(),
                        Axis::Vertical => this.flex_col(),
                    })
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                if let Err(error) = weak.update(cx, |view, _| {
                                    view.split_bounds.insert(recorded.clone(), bounds);
                                }) {
                                    eprintln!("bareframe: recording split bounds: {error}");
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .flex_none()
                            .min_w_0()
                            .min_h_0()
                            .map(|this| match axis {
                                Axis::Horizontal => this.h_full().w(relative(*ratio)),
                                Axis::Vertical => this.w_full().h(relative(*ratio)),
                            })
                            .child(first),
                    )
                    .child(divider)
                    .child(div().flex_1().min_w_0().min_h_0().child(second))
                    .into_any_element()
            }
        }
    }
}
