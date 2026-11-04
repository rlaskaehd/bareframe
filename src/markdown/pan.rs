//! Zooming into a Mermaid diagram and moving around it, as VS Code's preview does: the
//! diagram keeps its place in the document and grows inside it, or opens over the whole
//! pane. Pinch, or scroll with Option or Cmd held, to zoom; drag to move; double-click to
//! fit it again; Esc closes the whole-pane view.
//!
//! The picture is drawn at twice the diagram's own size. Zoomed past that, it is shown
//! enlarged while the zoom moves and, once it settles, the part in view is drawn again at
//! the screen's resolution: only that part, so a far zoom costs one screenful of pixels.
use super::{Diagram, MarkdownPane};
use crate::{theme, widgets};
use gpui::{
    AnyElement, Bounds, Context, MouseButton, MouseDownEvent, MouseMoveEvent, PinchEvent, Pixels,
    Point, RenderImage, ScrollWheelEvent, Size, canvas, div, img, prelude::*, px, relative,
};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

pub(super) const MAX_ZOOM: f32 = 8.0;

/// How long the view must stay put before the part in view is drawn again.
const SETTLE: Duration = Duration::from_millis(120);

/// A drawn diagram and how it is being looked at.
pub(super) struct Drawn {
    image: Arc<RenderImage>,
    /// Kept to draw the part in view again, sharp.
    svg: Arc<str>,
    view: View,
    /// That sharp drawing, with what it was drawn for.
    sharp: Option<(Sharp, Arc<RenderImage>)>,
}

impl Drawn {
    pub fn new(image: Arc<RenderImage>, svg: Arc<str>) -> Self {
        Self {
            image,
            svg,
            view: View::default(),
            sharp: None,
        }
    }

    /// The pictures GPUI holds for this diagram, to give back when it goes.
    pub fn into_images(self) -> impl Iterator<Item = Arc<RenderImage>> {
        std::iter::once(self.image).chain(self.sharp.map(|(_, image)| image))
    }

    /// Width over height, and the width in pixels.
    fn shape(&self) -> (f32, f32) {
        let size = self.image.size(0);
        let width = size.width.0 as f32;
        (width / size.height.0 as f32, width)
    }
}

/// What a sharp drawing of the part in view is for: the view, the box and the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sharp {
    view: View,
    size: Size<Pixels>,
    scale: f32,
}

impl Sharp {
    /// The picture's size in the box at the fitted zoom: `aspect` wide per unit high,
    /// fitted inside it.
    fn fitted(&self, aspect: f32) -> (f32, f32) {
        let (width, height) = (f32::from(self.size.width), f32::from(self.size.height));
        let across = width.min(height * aspect);
        (across, across / aspect)
    }

    /// The part of the diagram in the box, as fractions of it (see `diagram::crop`).
    fn part(&self, aspect: f32) -> [f32; 4] {
        let (width, height) = (f32::from(self.size.width), f32::from(self.size.height));
        let (fitted_width, fitted_height) = self.fitted(aspect);
        let View { zoom, x, y } = self.view;
        // The picture's top left corner in the box, and its size there.
        let left = x * width + zoom * (width - fitted_width) / 2.0;
        let top = y * height + zoom * (height - fitted_height) / 2.0;
        let (across, down) = (zoom * fitted_width, zoom * fitted_height);
        [-left / across, -top / down, width / across, height / down]
    }

    /// Whether the picture, `pixels` wide, would be enlarged on this screen.
    fn needed(&self, aspect: f32, pixels: f32) -> bool {
        self.view.zoom * self.fitted(aspect).0 * self.scale > pixels * 1.05
    }
}

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
    /// pane), zoomed and moved inside that box as its view says, with buttons to zoom, fit
    /// and open or close the whole-pane view; on the page they show under the pointer.
    pub(super) fn render_diagram(
        &self,
        source: &str,
        drawn: &Drawn,
        id: usize,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let source: Rc<str> = source.into();
        let (image, view) = (&drawn.image, drawn.view);
        let zoomed = view.zoom > 1.0;
        let sharp = drawn
            .sharp
            .as_ref()
            .filter(|(drawn_for, _)| drawn_for.view == view)
            .map(|(_, image)| image.clone());
        let has_sharp = drawn.sharp.as_ref().map(|(drawn_for, _)| *drawn_for);
        // Under the whole-pane view, the page's copy is not seen.
        let sharpens = expanded || self.expanded.is_none();
        let (aspect, pixels) = drawn.shape();
        let pane = cx.entity().downgrade();
        let redrawn = source.clone();
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
                            .when(zoomed || sharp.is_some(), |img| img.invisible()),
                    )
                    .when(zoomed && sharp.is_none(), |this| {
                        this.child(
                            img(image.clone())
                                .absolute()
                                .left(relative(view.x))
                                .top(relative(view.y))
                                .w(relative(view.zoom))
                                .h(relative(view.zoom)),
                        )
                    })
                    .when_some(sharp, |this, sharp| {
                        this.child(img(sharp).absolute().inset_0().size_full())
                    })
                    .child(
                        canvas(
                            move |box_bounds, window, cx| {
                                bounds.set(box_bounds);
                                let sharp = Sharp {
                                    view,
                                    size: box_bounds.size,
                                    scale: window.scale_factor(),
                                };
                                if sharpens
                                    && has_sharp != Some(sharp)
                                    && sharp.needed(aspect, pixels)
                                {
                                    cx.defer(move |cx| {
                                        pane.update(cx, |pane, cx| {
                                            pane.sharpen(&redrawn, sharp, cx)
                                        })
                                        .ok();
                                    });
                                }
                            },
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
                            } else if let Some(Diagram::Ready(drawn)) = pane.diagrams.get(&**source)
                                && drawn.view.zoom > 1.0
                            {
                                pane.panning = Some(Panning {
                                    source: source.clone(),
                                    start: event.position,
                                    from: drawn.view,
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
        if let Some(Diagram::Ready(drawn)) = self.diagrams.get_mut(source)
            && drawn.view != next
        {
            drawn.view = next;
            // It shows the view before; the enlarged picture stands in until it is redrawn.
            self.stale
                .extend(drawn.sharp.take().map(|(_, image)| image));
            cx.notify();
        }
    }

    fn zoom_diagram(&mut self, source: &str, factor: f32, at: (f32, f32), cx: &mut Context<Self>) {
        if let Some(Diagram::Ready(drawn)) = self.diagrams.get(source) {
            let next = drawn.view.zoomed(factor, at);
            self.set_view(source, next, cx);
        }
    }

    /// Draws the part of `source`'s diagram in view again, as `wanted` says, once the view
    /// has settled. A newer request replaces one still waiting.
    fn sharpen(&mut self, source: &str, wanted: Sharp, cx: &mut Context<Self>) {
        if self
            .sharpening
            .get(source)
            .is_some_and(|(drawing, _)| *drawing == wanted)
        {
            return;
        }
        let Some(Diagram::Ready(drawn)) = self.diagrams.get(source) else {
            return;
        };
        let (svg, part) = (drawn.svg.clone(), wanted.part(drawn.shape().0));
        let size = (f32::from(wanted.size.width), f32::from(wanted.size.height));
        let renderer = cx.svg_renderer();
        let key = source.to_owned();
        let task = cx.spawn(async move |pane, cx| {
            cx.background_executor().timer(SETTLE).await;
            let image = cx
                .background_executor()
                .spawn(async move {
                    let svg = crate::diagram::crop(&svg, part, size)?;
                    // GPUI draws an SVG at twice the scale it is given.
                    renderer
                        .render_single_frame(svg.as_bytes(), wanted.scale / 2.0)
                        .ok()
                })
                .await;
            pane.update(cx, |pane, cx| {
                pane.sharpening.remove(&key);
                if let Some(image) = image
                    && let Some(Diagram::Ready(drawn)) = pane.diagrams.get_mut(&key)
                    && drawn.view == wanted.view
                {
                    pane.stale
                        .extend(drawn.sharp.replace((wanted, image)).map(|(_, old)| old));
                    cx.notify();
                }
            })
            .ok();
        });
        self.sharpening.insert(source.to_owned(), (wanted, task));
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

    #[test]
    fn the_part_in_view_and_when_it_needs_drawing_again() {
        let at = |view, width, height| Sharp {
            view,
            size: gpui::size(px(width), px(height)),
            scale: 2.0,
        };
        let center = View::default().zoomed(2.0, (0.5, 0.5));
        // On the page the box is the picture's shape: the middle quarter is in view.
        assert_eq!(at(center, 400.0, 100.0).part(4.0), [0.25, 0.25, 0.5, 0.5]);
        // Over the whole pane the picture sits in the middle of a taller box.
        assert_eq!(
            at(View::default(), 400.0, 400.0).part(4.0),
            [0.0, -1.5, 1.0, 4.0]
        );
        // An 800 pixel picture fills a 400 point box on a 2x screen, until zoomed.
        assert!(!at(View::default(), 400.0, 100.0).needed(4.0, 800.0));
        assert!(at(center, 400.0, 100.0).needed(4.0, 800.0));
    }
}
