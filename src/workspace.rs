use crate::{
    app::{self, *},
    pane::{PaneEvent, TerminalPane},
    theme::{self, Theme, hsla},
};
use bareframe_core::{
    config::LaunchConfig,
    settings::{Settings, ThemeName},
    workspace::{Axis, Closed, Direction, Layout, PaneId, Workspace},
};
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Entity, FocusHandle, MouseButton, MouseMoveEvent,
    Pixels, Subscription, Window, WindowControlArea, canvas, div, prelude::*, px, relative,
};
use gpui_libghostty::TerminalTheme;
use std::collections::HashMap;

struct Resize {
    tab: usize,
    path: Vec<bool>,
    axis: Axis,
}

/// Owns tabs, layout and the panes; never a PTY or native surface directly.
pub struct WorkspaceView {
    model: Workspace,
    panes: HashMap<PaneId, (Entity<TerminalPane>, Subscription)>,
    config: LaunchConfig,
    settings: Settings,
    /// `Cmd+0` returns here: the size from the config file, not a hard-coded one.
    base_font_size: f32,
    resize: Option<Resize>,
    /// Split containers of the active tab, recorded during prepaint for resizing.
    split_bounds: HashMap<Vec<bool>, Bounds<Pixels>>,
    should_move_window: bool,
    focus: FocusHandle,
    _quit: Subscription,
}

impl WorkspaceView {
    pub fn new(
        config: LaunchConfig,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (model, first) = Workspace::new();
        let mut view = Self {
            model,
            panes: HashMap::new(),
            base_font_size: settings.font_size,
            settings,
            config,
            resize: None,
            split_bounds: HashMap::new(),
            should_move_window: false,
            focus: cx.focus_handle(),
            _quit: cx.on_app_quit(|view, cx| {
                view.shutdown(cx);
                async {}
            }),
        };
        let cwd = view.config.cwd.clone();
        view.open_pane(first, cwd, window, cx);
        view
    }

    fn theme(&self) -> &'static Theme {
        Theme::get(self.settings.theme)
    }

    fn terminal_theme(&self) -> TerminalTheme {
        self.theme().terminal(&self.settings)
    }

    fn open_pane(
        &mut self,
        id: PaneId,
        cwd: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let config = LaunchConfig {
            cwd,
            ..self.config.clone()
        };
        let theme = self.terminal_theme();
        let pane = cx.new(|cx| TerminalPane::new(id, config, &theme, window, cx));
        let subscription =
            cx.subscribe_in(
                &pane,
                window,
                move |view, _, event, window, cx| match event {
                    PaneEvent::Exited => view.close_pane(id, window, cx),
                    PaneEvent::TitleChanged => cx.notify(),
                },
            );
        self.panes.insert(id, (pane, subscription));
    }

    fn active_cwd(&self, cx: &Context<Self>) -> std::path::PathBuf {
        self.panes.get(&self.model.active_pane()).map_or_else(
            || self.config.cwd.clone(),
            |(pane, _)| pane.read(cx).cwd().to_path_buf(),
        )
    }

    /// Shows only the active tab's surfaces and gives the active pane keyboard focus.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible: Vec<PaneId> = self.model.active().layout.panes();
        for (id, (pane, _)) in &self.panes {
            let shown = visible.contains(id);
            pane.update(cx, |pane, cx| pane.set_visible(shown, cx));
        }
        if let Some((pane, _)) = self.panes.get(&self.model.active_pane()) {
            pane.update(cx, |pane, cx| pane.focus(window, cx));
        }
        self.split_bounds.clear();
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.active_cwd(cx);
        let pane = self.model.new_tab();
        self.open_pane(pane, cwd, window, cx);
        self.sync(window, cx);
    }

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.active_cwd(cx);
        let pane = self.model.split(axis);
        self.open_pane(pane, cwd, window, cx);
        self.sync(window, cx);
    }

    fn close_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((pane, _)) = self.panes.remove(&id) {
            pane.update(cx, |pane, cx| pane.shutdown(cx));
        }
        match self.model.close(id) {
            Some(Closed::LastTab) => {
                self.shutdown(cx);
                window.remove_window();
            }
            Some(_) => self.sync(window, cx),
            None => {}
        }
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.model.tabs.get(index) else {
            return;
        };
        for pane in tab.layout.panes() {
            self.close_pane(pane, window, cx);
        }
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.select_tab(index) {
            self.sync(window, cx);
        }
    }

    fn focus_direction(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.focus_direction(direction) {
            self.sync(window, cx);
        }
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let theme = self.terminal_theme();
        for (pane, _) in self.panes.values() {
            pane.update(cx, |pane, cx| pane.apply_theme(&theme, cx));
        }
        cx.notify();
    }

    fn set_theme(&mut self, name: ThemeName, cx: &mut Context<Self>) {
        self.settings.theme = name;
        self.apply_settings(cx);
    }

    fn step_font_size(&mut self, steps: f32, cx: &mut Context<Self>) {
        self.settings.step_font_size(steps);
        self.apply_settings(cx);
    }

    fn drag_divider(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(resize) = &self.resize else {
            return;
        };
        if !event.dragging() || resize.tab != self.model.active_tab {
            self.resize = None;
            return;
        }
        let Some(bounds) = self.split_bounds.get(&resize.path) else {
            return;
        };
        let ratio = match resize.axis {
            Axis::Horizontal => (event.position.x - bounds.origin.x) / bounds.size.width,
            Axis::Vertical => (event.position.y - bounds.origin.y) / bounds.size.height,
        };
        let layout = &mut self.model.tabs[resize.tab].layout;
        if layout.set_ratio(&resize.path, ratio) {
            cx.notify();
        }
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        for (_, (pane, _)) in self.panes.drain() {
            pane.update(cx, |pane, cx| pane.shutdown(cx));
        }
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let tabs = self.model.tabs.iter().enumerate().map(|(index, tab)| {
            let active = index == self.model.active_tab;
            let title = self
                .panes
                .get(&tab.active_pane)
                .map_or_else(String::new, |(pane, _)| pane.read(cx).title());
            let group = format!("tab-{index}");
            div()
                .id(("tab", index))
                .group(group.clone())
                .flex()
                .items_center()
                .gap_1()
                .h(px(28.0))
                .min_w(px(96.0))
                .max_w(px(220.0))
                .flex_1()
                .pl_3()
                .pr_1()
                .rounded_md()
                .text_color(hsla(if active { theme.text } else { theme.text_muted }))
                .when(active, |this| this.bg(hsla(theme.tab_active)))
                .when(!active, |this| {
                    this.hover(|style| style.bg(hsla(theme.tab_hover)))
                })
                .on_click(
                    cx.listener(move |view, _, window, cx| view.select_tab(index, window, cx)),
                )
                .child(div().flex_1().min_w_0().truncate().child(title))
                .child(
                    div()
                        .id(("close-tab", index))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(18.0))
                        .rounded_sm()
                        .text_color(hsla(theme.text_muted))
                        .hover(|style| style.bg(hsla(theme.border)).text_color(hsla(theme.text)))
                        .when(!active, |this| {
                            this.invisible().group_hover(group, |style| style.visible())
                        })
                        .on_click(cx.listener(move |view, _, window, cx| {
                            cx.stop_propagation();
                            view.close_tab(index, window, cx)
                        }))
                        .child("×"),
                )
        });
        div()
            .id("tab-bar")
            .window_control_area(WindowControlArea::Drag)
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .h(px(app::TAB_BAR_HEIGHT))
            // Leaves room for the traffic lights drawn over the transparent title bar.
            .pl(px(80.0))
            .pr_2()
            .bg(hsla(theme.chrome))
            .border_b_1()
            .border_color(hsla(theme.border))
            .font_family(theme::UI_FONT)
            .text_size(px(12.5))
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
            .children(tabs)
            .child(
                div()
                    .id("new-tab")
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(px(26.0))
                    .rounded_md()
                    .text_size(px(16.0))
                    .text_color(hsla(theme.text_muted))
                    .hover(|style| style.bg(hsla(theme.tab_hover)).text_color(hsla(theme.text)))
                    .on_click(cx.listener(|view, _, window, cx| {
                        cx.stop_propagation();
                        view.new_tab(window, cx)
                    }))
                    .child("+"),
            )
    }

    fn render_layout(
        &self,
        layout: &Layout,
        path: &mut Vec<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme();
        match layout {
            Layout::Pane(id) => {
                let id = *id;
                let active =
                    id == self.model.active_pane() && self.model.active().layout.panes().len() > 1;
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    // Breathing room around the text; the surface can't be padded from GPUI.
                    .p(px(6.0))
                    .border_1()
                    .border_color(hsla(if active {
                        theme.accent
                    } else {
                        theme.terminal_background
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, window, cx| {
                            if view.model.active_pane() != id && view.model.focus(id) {
                                view.sync(window, cx);
                            }
                        }),
                    )
                    .children(self.panes.get(&id).map(|(pane, _)| pane.clone()))
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

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let title = self
            .panes
            .get(&self.model.active_pane())
            .map_or_else(String::new, |(pane, _)| pane.read(cx).title());
        window.set_window_title(&format!("{title} — {}", app::name()));
        let layout = self.model.active().layout.clone();
        let content = self.render_layout(&layout, &mut Vec::new(), cx);
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.terminal_background))
            .text_color(hsla(theme.text))
            .on_mouse_move(cx.listener(|view, event, _, cx| view.drag_divider(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.resize = None),
            )
            .on_action(cx.listener(|view, _: &CloseWindow, window, cx| {
                view.shutdown(cx);
                window.remove_window();
            }))
            .on_action(cx.listener(|view, _: &ClosePane, window, cx| {
                let pane = view.model.active_pane();
                view.close_pane(pane, window, cx)
            }))
            .on_action(cx.listener(|view, _: &RestartSession, window, cx| {
                let theme = view.terminal_theme();
                if let Some((pane, _)) = view.panes.get(&view.model.active_pane()) {
                    pane.update(cx, |pane, cx| pane.retry(&theme, window, cx));
                }
            }))
            .on_action(cx.listener(|view, _: &NewTab, window, cx| view.new_tab(window, cx)))
            .on_action(cx.listener(|view, _: &NextTab, window, cx| {
                view.model.cycle_tab(true);
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|view, _: &PreviousTab, window, cx| {
                view.model.cycle_tab(false);
                view.sync(window, cx)
            }))
            .on_action(cx.listener(|view, _: &SelectTab1, w, cx| view.select_tab(0, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab2, w, cx| view.select_tab(1, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab3, w, cx| view.select_tab(2, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab4, w, cx| view.select_tab(3, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab5, w, cx| view.select_tab(4, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab6, w, cx| view.select_tab(5, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab7, w, cx| view.select_tab(6, w, cx)))
            .on_action(cx.listener(|view, _: &SelectTab8, w, cx| view.select_tab(7, w, cx)))
            .on_action(cx.listener(|view, _: &SelectLastTab, window, cx| {
                let last = view.model.tabs.len() - 1;
                view.select_tab(last, window, cx)
            }))
            .on_action(cx.listener(|view, _: &SplitRight, window, cx| {
                view.split(Axis::Horizontal, window, cx)
            }))
            .on_action(
                cx.listener(|view, _: &SplitDown, window, cx| {
                    view.split(Axis::Vertical, window, cx)
                }),
            )
            .on_action(cx.listener(|view, _: &FocusLeft, w, cx| {
                view.focus_direction(Direction::Left, w, cx)
            }))
            .on_action(cx.listener(|view, _: &FocusRight, w, cx| {
                view.focus_direction(Direction::Right, w, cx)
            }))
            .on_action(
                cx.listener(|view, _: &FocusUp, w, cx| view.focus_direction(Direction::Up, w, cx)),
            )
            .on_action(cx.listener(|view, _: &FocusDown, w, cx| {
                view.focus_direction(Direction::Down, w, cx)
            }))
            .on_action(
                cx.listener(|view, _: &IncreaseFontSize, _, cx| view.step_font_size(1.0, cx)),
            )
            .on_action(
                cx.listener(|view, _: &DecreaseFontSize, _, cx| view.step_font_size(-1.0, cx)),
            )
            .on_action(cx.listener(|view, _: &ResetFontSize, _, cx| {
                view.settings.font_size = view.base_font_size;
                view.apply_settings(cx)
            }))
            .on_action(
                cx.listener(|view, _: &UseDarkTheme, _, cx| view.set_theme(ThemeName::Dark, cx)),
            )
            .on_action(
                cx.listener(|view, _: &UseLightTheme, _, cx| view.set_theme(ThemeName::Light, cx)),
            )
            .child(self.render_tab_bar(cx))
            .child(div().flex_1().min_h_0().child(content))
    }
}
