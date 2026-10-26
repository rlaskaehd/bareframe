//! The vertical tabs sidebar: search, groups, tab cards, renaming and drag and drop.
use super::{RenameTarget, WorkspaceView, icon_button};
use crate::{
    rename::{RenameEvent, RenameInput},
    theme::{self, Theme, hsla},
};
use bareframe_core::workspace::{GroupId, Tab};
use gpui::{AnyElement, Context, Hsla, MouseButton, MouseDownEvent, Window, div, prelude::*, px};

/// Sidebar width bounds follow Warp's vertical tabs panel.
pub(super) const SIDEBAR_DEFAULT_WIDTH: f32 = 248.0;
pub(super) const SIDEBAR_MIN_WIDTH: f32 = 200.0;
pub(super) const SIDEBAR_MAX_SHARE: f32 = 0.5;

/// Tabs being dragged in the sidebar (the selection, or one tab); also renders the preview.
#[derive(Clone)]
struct DraggedTab {
    indices: Vec<usize>,
    title: String,
    background: u32,
    text: u32,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(hsla(self.background))
            .text_color(hsla(self.text))
            .font_family(theme::UI_FONT)
            .text_size(px(12.5))
            .child(self.title.clone())
    }
}

impl WorkspaceView {
    /// Drops `dragged` before the tab at `to`, joining `group`.
    fn drop_tab(
        &mut self,
        dragged: &DraggedTab,
        to: usize,
        group: Option<GroupId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.move_tabs(&dragged.indices, to, group);
        self.sync(window, cx);
    }

    /// Plain click focuses a tab; shift extends the selection from the active tab and
    /// cmd toggles one tab, as in Warp's vertical tabs.
    fn click_tab(
        &mut self,
        index: usize,
        modifiers: gpui::Modifiers,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if modifiers.shift {
            self.model.select_range(index);
            cx.notify();
        } else if click_count == 2 {
            if let Some(tab) = self.model.tabs.get(index) {
                self.start_rename(RenameTarget::Tab(tab.id), window, cx);
            }
        } else if modifiers.platform {
            self.model.toggle_selected(index);
            cx.notify();
        } else {
            self.model.clear_selection();
            self.select_tab(index, window, cx);
        }
    }

    fn start_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }
        let accent = self.theme().accent;
        let input = cx.new(|cx| RenameInput::search(accent, window, cx));
        let events = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            view.finish_search(event, window, cx)
        });
        let changes = cx.observe(&input, |_, _, cx| cx.notify());
        self.search = Some((input, [events, changes]));
        cx.notify();
    }

    /// Enter jumps to the first match; Esc clears the filter.
    fn finish_search(&mut self, event: &RenameEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let RenameEvent::Confirm(_) = event
            && let Some(matches) = self.search_matches(cx)
        {
            let Some(&first) = matches.first() else {
                return;
            };
            self.model.clear_selection();
            self.model.select_tab(first);
        }
        self.search = None;
        self.sync(window, cx);
    }

    /// Tabs whose title or path contains the query, or `None` when not searching.
    fn search_matches(&self, cx: &Context<Self>) -> Option<Vec<usize>> {
        let (input, _) = self.search.as_ref()?;
        let query = input.read(cx).text().trim().to_lowercase();
        if query.is_empty() {
            return None;
        }
        let found = |text: String| text.to_lowercase().contains(&query);
        Some(
            (0..self.model.tabs.len())
                .filter(|&index| {
                    let tab = &self.model.tabs[index];
                    found(self.tab_title(tab, cx)) || self.tab_subtitle(tab, cx).is_some_and(found)
                })
                .collect(),
        )
    }

    pub(super) fn group_label(&self, id: GroupId) -> String {
        self.model
            .group(id)
            .and_then(|group| group.name.clone())
            .unwrap_or_else(|| "New Group".into())
    }

    pub(super) fn start_rename(
        &mut self,
        target: RenameTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match target {
            RenameTarget::Tab(id) => match self.model.tab_index(id) {
                Some(index) => self.tab_title(&self.model.tabs[index], cx),
                None => return,
            },
            RenameTarget::Group(id) => match self.model.group(id) {
                Some(group) => group.name.clone().unwrap_or_default(),
                None => return,
            },
        };
        let accent = self.theme().accent;
        let input = cx.new(|cx| RenameInput::new(&text, accent, window, cx));
        let subscription = cx.subscribe_in(&input, window, move |view, _, event, window, cx| {
            view.finish_rename(target, event, window, cx)
        });
        self.renaming = Some((target, input, subscription));
        cx.notify();
    }

    fn finish_rename(
        &mut self,
        target: RenameTarget,
        event: &RenameEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.renaming.as_ref().map(|(current, _, _)| *current) != Some(target) {
            return;
        }
        self.renaming = None;
        if let RenameEvent::Confirm(text) = event {
            match target {
                RenameTarget::Tab(id) => self.model.rename_tab(id, text),
                RenameTarget::Group(id) => self.model.rename_group(id, text),
            }
        }
        self.sync(window, cx);
    }

    /// The rename field for `target`, if it is being renamed.
    fn rename_field(&self, target: RenameTarget) -> Option<AnyElement> {
        let (current, input, _) = self.renaming.as_ref()?;
        let theme = self.theme();
        (*current == target).then(|| {
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .px_1()
                .rounded_sm()
                .bg(hsla(theme.terminal_background))
                .border_1()
                .border_color(hsla(theme.accent))
                .text_color(hsla(theme.text))
                .child(input.clone())
                .into_any_element()
        })
    }

    /// A custom title wins over the shell's, as in Warp.
    fn tab_title(&self, tab: &Tab, cx: &Context<Self>) -> String {
        tab.title.clone().unwrap_or_else(|| {
            self.pane(tab.active_pane)
                .map_or_else(String::new, |pane| pane.read(cx).name())
        })
    }

    /// The second line of a tab row: the active pane's `~` path.
    fn tab_subtitle(&self, tab: &Tab, cx: &Context<Self>) -> Option<String> {
        Some(self.pane(tab.active_pane)?.read(cx).display_cwd())
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let mut rows = Vec::new();
        if let Some(matches) = self.search_matches(cx) {
            // A filtered list is flat: groups and the pin divider would only get in the way.
            if matches.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .py_2()
                        .text_color(hsla(theme.text_muted))
                        .child("No matching tabs")
                        .into_any_element(),
                );
            }
            for index in matches {
                rows.push(self.render_tab_row(index, &self.model.tabs[index], cx));
            }
        } else {
            let mut previous = None;
            for (index, tab) in self.model.tabs.iter().enumerate() {
                if index > 0 && self.model.tab_pinned(index - 1) && !self.model.tab_pinned(index) {
                    rows.push(
                        div()
                            .flex_shrink_0()
                            .h(px(1.0))
                            .bg(hsla(theme.text_muted).opacity(0.5))
                            .into_any_element(),
                    );
                }
                if tab.group != previous {
                    if let Some(group) = tab.group {
                        rows.push(self.render_group_header(group, cx));
                    }
                    previous = tab.group;
                }
                let collapsed = tab
                    .group
                    .and_then(|group| self.model.group(group))
                    .is_some_and(|group| group.collapsed);
                if !collapsed {
                    rows.push(self.render_tab_row(index, tab, cx));
                }
            }
        }
        let end = self.model.tabs.len();
        let search = self.search.as_ref().map(|(input, _)| input.clone());
        let searching = search
            .as_ref()
            .is_some_and(|input| !input.read(cx).text().is_empty());
        div()
            .w(px(self.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.chrome))
            .font_family(theme::UI_FONT)
            .text_size(px(12.5))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .h(px(40.0))
                    .pl_3()
                    .pr_2()
                    .border_b_1()
                    .border_color(hsla(theme.border))
                    .child(
                        gpui::svg()
                            .path("icons/search.svg")
                            .flex_shrink_0()
                            .size(px(14.0))
                            .text_color(hsla(theme.text_muted)),
                    )
                    .child(
                        div()
                            .id("tab-search")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .cursor_text()
                            .text_color(hsla(theme.text))
                            .on_click(
                                cx.listener(|view, _, window, cx| view.start_search(window, cx)),
                            )
                            .when(!searching, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .text_color(hsla(theme.text_muted))
                                        .child("Search tabs..."),
                                )
                            })
                            .children(search),
                    )
                    .child(
                        icon_button("new-tab", "+", theme)
                            .text_size(px(16.0))
                            .on_click(cx.listener(|view, _, window, cx| {
                                cx.stop_propagation();
                                view.new_tab(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("tab-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(rows)
                    // Dropping below the list moves a tab to the end, outside any group.
                    .child(
                        div()
                            .id("tab-list-end")
                            .flex_1()
                            .min_h(px(32.0))
                            .drag_over::<DraggedTab>(move |style, _, _, _| {
                                style.bg(hsla(theme.tab_hover))
                            })
                            .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                                view.drop_tab(dragged, end, None, window, cx)
                            })),
                    ),
            )
    }

    fn render_group_header(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let Some(group) = self.model.group(id) else {
            return div().into_any_element();
        };
        let tabs = self.model.group_tabs(id);
        let end = tabs.last().map_or(0, |last| last + 1);
        let hover = format!("group-{}", id.0);
        let color = hsla(theme.group_colors[group.color]);
        // A collapsed group hides the active tab's row, so the header stands in for it.
        let holds_active = group.collapsed && tabs.contains(&self.model.active_tab);
        div()
            .id(("group", id.0))
            .group(hover.clone())
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .min_h(px(34.0))
            .mt_1()
            .px_2()
            .rounded_md()
            .text_color(hsla(theme.text))
            .when(holds_active, |this| this.bg(hsla(theme.tab_active)))
            .hover(|style| style.bg(hsla(theme.tab_hover)))
            .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(hsla(theme.tab_active)))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.model.toggle_group(id);
                cx.notify()
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.open_group_menu(id, event.position, cx)
                }),
            )
            .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                view.drop_tab(dragged, end, Some(id), window, cx)
            }))
            .child(
                div()
                    .w(px(10.0))
                    .text_color(hsla(theme.text_muted))
                    .child(if group.collapsed { "▸" } else { "▾" }),
            )
            .child(div().size(px(8.0)).rounded_full().bg(color))
            .child(match self.rename_field(RenameTarget::Group(id)) {
                Some(field) => field,
                None => div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.group_label(id)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(hsla(theme.text_muted))
                            .child(match tabs.len() {
                                1 => "1 tab".to_owned(),
                                count => format!("{count} tabs"),
                            }),
                    )
                    .into_any_element(),
            })
            .when(group.pinned, |this| this.child(pin_mark(theme)))
            .child(
                div()
                    .flex()
                    .invisible()
                    .group_hover(hover, |style| style.visible())
                    .child(
                        icon_button(("group-new-tab", id.0), "+", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.new_tab_in_group(id, window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(("group-close", id.0), "×", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_tabs(&view.model.group_tabs(id), window, cx)
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    /// A Warp-style session card: terminal icon, title and path, separated by lines, with
    /// a menu and close button floating on hover.
    fn render_tab_row(&self, index: usize, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme();
        let active = index == self.model.active_tab;
        let title = self.tab_title(tab, cx);
        let subtitle = self.tab_subtitle(tab, cx);
        // Selection highlight only matters once more than one tab is involved.
        let selection = self.model.selected_indices();
        let selected = selection.len() > 1 && selection.contains(&index);
        let group = tab.group;
        let group_color = group
            .and_then(|group| self.model.group(group))
            .map(|group| hsla(theme.group_colors[group.color]));
        let tint = tab.color.map(|color| hsla(theme.group_colors[color]));
        let hover = format!("tab-{index}");
        let dragged = DraggedTab {
            title: if selected {
                format!("{} tabs", selection.len())
            } else {
                title.clone()
            },
            indices: if selected {
                selection.clone()
            } else {
                vec![index]
            },
            background: theme.tab_active,
            text: theme.text,
        };
        let card = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .text_color(hsla(if active || selected {
                theme.text
            } else {
                theme.text_muted
            }))
            .map(|this| match (tint, active) {
                (Some(tint), _) => this.bg(tint.opacity(if active { 0.3 } else { 0.15 })),
                (None, true) => this.bg(hsla(theme.tab_active)),
                (None, false) => {
                    this.group_hover(hover.clone(), |style| style.bg(hsla(theme.tab_hover)))
                }
            })
            .when(active, |this| {
                this.border_color(hsla(theme.text).opacity(0.2))
            })
            .when(selected, |this| {
                this.bg(hsla(theme.accent).opacity(0.22))
                    .border_color(hsla(theme.accent).opacity(0.5))
            })
            .when_some(group_color, |this, color| {
                this.child(div().w(px(3.0)).h(px(26.0)).rounded_sm().bg(color))
            })
            .child(terminal_icon(theme, active, tint))
            .child(match self.rename_field(RenameTarget::Tab(tab.id)) {
                Some(field) => field,
                None => div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(px(13.0)).child(title))
                    .when_some(subtitle, |this, subtitle| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(hsla(theme.text_muted))
                                .child(subtitle),
                        )
                    })
                    .into_any_element(),
            })
            .when(group.is_none() && tab.pinned, |this| {
                this.child(pin_mark(theme))
            });
        div()
            .id(("tab", index))
            .group(hover.clone())
            .relative()
            .flex_shrink_0()
            .p(px(6.0))
            .when(group.is_some(), |this| this.pl(px(14.0)))
            .border_b_1()
            .border_color(hsla(theme.border))
            .on_click(
                cx.listener(move |view, event: &gpui::ClickEvent, window, cx| {
                    view.click_tab(index, event.modifiers(), event.click_count(), window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.open_tab_menu(index, event.position, cx)
                }),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<DraggedTab>(move |style, _, _, _| {
                style.border_t_2().border_color(hsla(theme.accent))
            })
            .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
                view.drop_tab(dragged, index, group, window, cx)
            }))
            .child(card)
            .child(
                div()
                    .absolute()
                    .top(px(2.0))
                    .right(px(4.0))
                    .flex()
                    .gap(px(2.0))
                    .p(px(2.0))
                    .rounded_md()
                    .bg(hsla(theme.chrome))
                    .border_1()
                    .border_color(hsla(theme.border))
                    .invisible()
                    .group_hover(hover, |style| style.visible())
                    .child(
                        icon_button(("tab-menu", index), "⋮", theme).on_click(cx.listener(
                            move |view, event: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                view.open_tab_menu(index, event.position(), cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(("close-tab", index), "×", theme).on_click(cx.listener(
                            move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_tabs(&[index], window, cx)
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

/// Warp's session icon: a prompt glyph in a circle, in the tab's color if any.
fn terminal_icon(theme: &Theme, active: bool, tint: Option<Hsla>) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(28.0))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(match tint {
            Some(tint) => tint.opacity(0.25),
            None => hsla(if active {
                theme.border
            } else {
                theme.tab_active
            }),
        })
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(tint.unwrap_or(hsla(theme.text)))
        .child(">_")
}

fn pin_mark(theme: &Theme) -> impl IntoElement {
    gpui::svg()
        .path("icons/pin.svg")
        .flex_shrink_0()
        .size(px(12.0))
        .text_color(hsla(theme.text_muted))
}
