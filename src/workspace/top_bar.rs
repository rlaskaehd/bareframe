//! The bar along the window's top, in place of the system title bar.
use super::WorkspaceView;
use crate::{app, theme::hsla};
use gpui::{Context, MouseButton, WindowControlArea, div, prelude::*, px};

impl WorkspaceView {
    /// Full-width bar under the traffic lights. The middle is reserved for a search box.
    /// It stands in for the hidden system title bar: drag moves the window, double-click zooms.
    pub(super) fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let open = self.sidebar_open;
        div()
            .id("top-bar")
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.should_move_window = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.should_move_window = false),
            )
            .on_mouse_move(cx.listener(|view, _, window, _| {
                if view.should_move_window {
                    view.should_move_window = false;
                    window.start_window_move();
                }
            }))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .flex()
            .flex_shrink_0()
            .items_center()
            .h(px(app::TITLE_BAR_HEIGHT))
            // Leaves room for the traffic lights drawn over the transparent title bar.
            .pl(px(80.0))
            .pr_2()
            .bg(hsla(theme.chrome))
            .border_b_1()
            .border_color(hsla(theme.border))
            .child(
                div()
                    .id("toggle-sidebar")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .when(open, |this| this.bg(hsla(theme.tab_active)))
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(|view, _, _, cx| {
                        cx.stop_propagation();
                        view.toggle_sidebar(cx)
                    }))
                    .child(sidebar_icon(if open {
                        theme.text
                    } else {
                        theme.text_muted
                    })),
            )
            .child(
                div()
                    .id("toggle-tools")
                    // Hidden when Settings has switched every tool off.
                    .when(self.shown_tool().is_none(), |this| this.invisible())
                    .ml_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .when(self.tools_open, |this| this.bg(hsla(theme.tab_active)))
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(|view, _, window, cx| {
                        cx.stop_propagation();
                        view.toggle_tools(window, cx)
                    }))
                    .child(
                        gpui::svg()
                            .path("icons/wrench.svg")
                            .size(px(14.0))
                            .text_color(hsla(if self.tools_open {
                                theme.text
                            } else {
                                theme.text_muted
                            })),
                    ),
            )
            .child(div().flex_1())
            .children(self.render_update_chip(cx))
            .children(self.render_diff_button(cx))
            .child(
                div()
                    .id("open-settings")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(|view, _, window, cx| {
                        cx.stop_propagation();
                        view.open_settings(window, cx)
                    }))
                    .child(
                        gpui::svg()
                            .path("icons/gear.svg")
                            .size(px(15.0))
                            .text_color(hsla(theme.text_muted)),
                    ),
            )
    }

    /// A new release on offer, the install under way, or how a check from the menu ended.
    fn render_update_chip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        use crate::update::State;
        let theme = self.theme();
        let updates = crate::update::global(cx);
        let (label, color) = match &updates.read(cx).state {
            State::Idle | State::Checking => return None,
            State::Available(release) => (format!("Update to v{}", release.version), theme.accent),
            State::Installing(version) => (format!("Installing v{version}…"), theme.text_muted),
            State::UpToDate => ("bareframe is up to date".to_owned(), theme.text_muted),
            State::Failed(error) => (format!("Update failed: {error}"), theme.text_muted),
        };
        Some(
            div()
                .id("update-chip")
                .flex()
                .items_center()
                .mr_2()
                .px_2()
                .h(px(24.0))
                .max_w(px(360.0))
                .overflow_hidden()
                .rounded_md()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(hsla(color))
                .hover(|style| style.bg(hsla(theme.tab_hover)))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    updates.update(cx, |updates, cx| match updates.state {
                        State::Available(_) => updates.install(cx),
                        _ => updates.dismiss(cx),
                    })
                })
                .child(div().truncate().child(label)),
        )
    }
}

/// Outline of a window with its left panel filled, drawn so no icon font is needed.
fn sidebar_icon(color: u32) -> impl IntoElement {
    div()
        .flex()
        .w(px(15.0))
        .h(px(12.0))
        .rounded(px(2.5))
        .border_1()
        .border_color(hsla(color))
        .child(div().w(px(4.5)).h_full().bg(hsla(color)))
}
