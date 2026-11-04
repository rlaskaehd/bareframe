//! Zooming into a Mermaid diagram and moving around it, as VS Code's preview does: the
//! diagram keeps its place in the document and grows inside it, or opens over the whole
//! pane. Pinch, or scroll with Option or Cmd held, to zoom; drag to move; double-click to
//! fit it again; Esc closes the whole-pane view.
use super::{Diagram, MarkdownPane};
use crate::{theme, widgets};
use gpui::{
    AnyElement, Bounds, Context, MouseButton, MouseDownEvent, MouseMoveEvent, PinchEvent, Pixels,
    Point, RenderImage, ScrollWheelEvent, Size, canvas, div, img, prelude::*, px, relative,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

/// The picture is drawn at twice the diagram's own size, so it stays sharp until a diagram
/// squeezed to the page is back at full size; past that it is enlarged.
// ponytail: no redraw at the zoomed scale; re-rasterize with `SvgRenderer::render_parsed`
// if diagrams small enough to fit the page are read zoomed far in.
pub(super) const MAX_ZOOM: f32 = 8.0;

/// `zoom` times the fitted size, its top left corner at `x`, `y` in fractions of the box:
/// zero or less, so the picture always covers the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct View {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

impl View {
    /// Zoomed by `factor`, keeping the point `at` (fractions of the box) where it is.
    pub fn zoomed(self, factor: f32, at: (f32, f32)) -> Self {
        let zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
        let keep = |corner: f32, at: f32| at - (at - corner) / self.zoom * zoom;
        Self {
            zoom,
            x: keep(self.x, at.0),
            y: keep(self.y, at.1),
        }
        .clamped()
    }

    pub fn moved_to(self, x: f32, y: f32) -> Self {
        Self { x, y, ..self }.clamped()
    }

    fn clamped(self) -> Self {
        let far = 1.0 - self.zoom;
        Self {
            x: self.x.clamp(far, 0.0),
            y: self.y.clamp(far, 0.0),
            ..self
        }
    }
}

/// A drag on a zoomed diagram: where it started and how the diagram lay then.
pub(super) struct Panning {
    source: Rc<str>,
    start: Point<Pixels>,
    from: View,
    size: Size<Pixels>,
}

/// Where `position` falls in `bounds`, as fractions of its size.
fn fraction(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
    let at = position - bounds.origin;
    (at.x / bounds.size.width, at.y / bounds.size.height)
}

impl MarkdownPane {
    /// The diagram drawn from `source`: fitted to the page (or, `expanded`, to the whole
    /// pane), zoomed and moved inside that box as `view` says, with buttons to zoom, fit and
    /// open or close the whole-pane view; on the page they show under the pointer.
    pub(super) fn render_diagram(
        &self,
        source: &str,
        image: &Arc<RenderImage>,
        view: View,
        id: usize,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let source: Rc<str> = source.into();
        let zoomed = view.zoom > 1.0;
        // The box's place on screen, from this frame's layout, for the next events.
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let group = format!("md-diagram-{id}");
        type Act = fn(&mut MarkdownPane, &str, &mut Context<MarkdownPane>);
        let button = |name: &'static str, label: &'static str, act: Act| {
            let source = source.clone();
            widgets::icon_button(theme, (name, id), label)
                // Not a drag or a double-click on the diagram under it.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |pane, _, _, cx| act(pane, &source, cx)))
        };
        // Fit comes first, so − and + stay where they were when it appears.
        let controls = div()
            .absolute()
            .top_1()
            .right_1()
            .flex()
            .gap_0p5()
            .p_0p5()
            .rounded_md()
            .bg(theme::hsla(theme.tab_active))
            .border_1()
            .border_color(theme::hsla(theme.border))
            .when(!expanded, |this| {
                this.invisible()
                    .group_hover(group.clone(), |style| style.visible())
            })
            .when(zoomed, |this| {
                this.child(button("md-zoom-fit", "↺", |pane, source, cx| {
                    pane.set_view(source, View::default(), cx)
                }))
            })
            .child(button("md-zoom-out", "−", |pane, source, cx| {
                pane.zoom_diagram(source, 1.0 / 1.25, (0.5, 0.5), cx)
            }))
            .child(button("md-zoom-in", "+", |pane, source, cx| {
                pane.zoom_diagram(source, 1.25, (0.5, 0.5), cx)
            }))
            .child(if expanded {
                button("md-collapse", "×", |pane, _, cx| pane.close_diagram(cx))
            } else {
                button("md-expand", "", |pane, source, cx| {
                    pane.expanded = Some(source.to_owned());
                    cx.notify();
                })
                .child(widgets::icon(theme, "icons/maximize-2.svg"))
            });
        let pinched = (source.clone(), bounds.clone());
        let scrolled = (source.clone(), bounds.clone());
        let pressed = (source, bounds.clone());
        div()
            .when(expanded, |this| this.size_full())
            .when(!expanded, |this| this.w_full().flex().justify_center())
            .child(
                div()
                    .id(("md-diagram", id))
                    .group(group)
                    .relative()
                    .when(expanded, |this| this.size_full())
                    .when(!expanded, |this| this.max_w_full())
                    .overflow_hidden()
                    .when(zoomed, |this| this.cursor_grab())
                    // Lays the box out at the fitted size; the zoomed copy covers it. In the
                    // whole pane the box is the pane and the picture is fitted inside it, so a
                    // place in the box is still one place in the diagram.
                    .child(
                        img(image.clone())
                            .when(expanded, |img| img.size_full())
                            .when(!expanded, |img| img.max_w_full())
                            .when(zoomed, |img| img.invisible()),
                    )
                    .when(zoomed, |this| {
                        this.child(
                            img(image.clone())
                                .absolute()
                                .left(relative(view.x))
                                .top(relative(view.y))
                                .w(relative(view.zoom))
                                .h(relative(view.zoom)),
                        )
                    })
                    .child(
                        canvas(
                            move |box_bounds, _, _| bounds.set(box_bounds),
                            |_, _, _, _| {},
                        )
                        // Pinned to the box: left to itself it would sit below the picture.
                        .absolute()
                        .inset_0(),
                    )
                    .child(controls)
                    .on_pinch(cx.listener(move |pane, event: &PinchEvent, _, cx| {
                        let (source, bounds) = &pinched;
                        let at = fraction(bounds.get(), event.position);
                        pane.zoom_diagram(source, 1.0 + event.delta, at, cx);
                    }))
                    .on_scroll_wheel(cx.listener(
                        move |pane, event: &ScrollWheelEvent, window, cx| {
                            // A plain scroll still moves the document.
                            if !(event.modifiers.alt || event.modifiers.platform) {
                                return;
                            }
                            cx.stop_propagation();
                            let (source, bounds) = &scrolled;
                            let delta = event.delta.pixel_delta(window.line_height()).y;
                            let at = fraction(bounds.get(), event.position);
                            pane.zoom_diagram(source, (delta / px(120.0)).exp(), at, cx);
                        },
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |pane, event: &MouseDownEvent, window, cx| {
                            let (source, bounds) = &pressed;
                            pane.focus.focus(window, cx);
                            if event.click_count == 2 {
                                pane.set_view(source, View::default(), cx);
                            } else if let Some(Diagram::Ready(_, from)) =
                                pane.diagrams.get(&**source)
                                && from.zoom > 1.0
                            {
                                pane.panning = Some(Panning {
                                    source: source.clone(),
                                    start: event.position,
                                    from: *from,
                                    size: bounds.get().size,
                                });
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Back to the page, the diagram fitted again.
    pub(super) fn close_diagram(&mut self, cx: &mut Context<Self>) {
        match self.expanded.take() {
            Some(source) => {
                self.set_view(&source, View::default(), cx);
                cx.notify();
            }
            None => cx.propagate(),
        }
    }

    fn set_view(&mut self, source: &str, next: View, cx: &mut Context<Self>) {
        if let Some(Diagram::Ready(_, view)) = self.diagrams.get_mut(source)
            && *view != next
        {
            *view = next;
            cx.notify();
        }
    }

    fn zoom_diagram(&mut self, source: &str, factor: f32, at: (f32, f32), cx: &mut Context<Self>) {
        if let Some(Diagram::Ready(_, view)) = self.diagrams.get(source) {
            let next = view.zoomed(factor, at);
            self.set_view(source, next, cx);
        }
    }

    /// Moves the diagram being dragged; false when none is.
    pub(super) fn pan_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        let Some(panning) = &self.panning else {
            return false;
        };
        if event.pressed_button != Some(MouseButton::Left) || panning.size.width <= px(0.0) {
            self.panning = None;
            return true;
        }
        let moved = event.position - panning.start;
        let next = panning.from.moved_to(
            panning.from.x + moved.x / panning.size.width,
            panning.from.y + moved.y / panning.size.height,
        );
        let source = panning.source.clone();
        self.set_view(&source, next, cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_point_under_the_pointer_and_the_box_covered() {
        let center = View::default().zoomed(2.0, (0.5, 0.5));
        assert_eq!(
            center,
            View {
                zoom: 2.0,
                x: -0.5,
                y: -0.5
            }
        );
        assert_eq!(center.zoomed(0.5, (0.5, 0.5)), View::default());
        // The top left corner stays put, and moving can't pull an edge inside the box.
        let corner = View::default().zoomed(4.0, (0.0, 0.0));
        assert_eq!((corner.x, corner.y), (0.0, 0.0));
        assert_eq!(
            corner.moved_to(0.3, -9.0),
            View {
                zoom: 4.0,
                x: 0.0,
                y: -3.0
            }
        );
        assert_eq!(View::default().zoomed(0.5, (0.2, 0.2)), View::default());
        assert_eq!(View::default().zoomed(100.0, (0.0, 0.0)).zoom, MAX_ZOOM);
    }
}
