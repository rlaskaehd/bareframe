//! The active tab's split layout and the pane headers.
use super::{Resize, WorkspaceView, icon_button};
use crate::theme::{self, hsla};
use bareframe_core::workspace::{Axis, Layout, PaneId};
use gpui::{AnyElement, Context, CursorStyle, MouseButton, canvas, div, prelude::*, px, relative};

impl WorkspaceView {
    /// Shown once a tab is split, as in Warp: the title, a menu and a close button, with a
    /// corner mark on the focused pane.
    fn render_pane_header(&self, id: PaneId, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let active = id == self.model.active_pane();
        let title = self
            .panes
            .get(&id)
            .map_or_else(String::new, |(pane, _)| pane.read(cx).title());
        let key = id.0 as usize;
        div()
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
                let pane = self.panes.get(&id).map(|(pane, _)| pane.clone());
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
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
                            .p(px(6.0))
                            .children(pane),
                    )
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
