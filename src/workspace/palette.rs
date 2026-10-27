//! The command palette (`Cmd+Shift+P`): every keymap action with its shortcuts, the
//! Settings sections and the open tabs, found by fuzzy name and run with Enter.
use super::{PaneView, WorkspaceView};
use crate::{
    app,
    settings_pane::{self, Section},
    text_field::{TextField, TextFieldEvent},
    theme::hsla,
};
use bareframe_core::{
    fuzzy::fuzzy_match,
    keymap::{self, ACTIONS, Trigger},
};
use gpui::{
    Context, Entity, FontWeight, HighlightStyle, KeyDownEvent, ScrollHandle, StyledText,
    Subscription, Window, div, prelude::*, px,
};

const WIDTH: f32 = 600.0;
const LIST_HEIGHT: f32 = 360.0;

pub(super) struct Palette {
    input: Entity<TextField>,
    selected: usize,
    scroll: ScrollHandle,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone, Copy)]
enum Command {
    Action(&'static str),
    Settings(Section),
    Tab(usize),
}

struct Item {
    label: String,
    detail: Option<String>,
    triggers: Vec<Trigger>,
    command: Command,
    /// Byte offsets of the label's letters the query matched.
    matched: Vec<usize>,
}

impl WorkspaceView {
    pub(super) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            return self.close_palette(window, cx);
        }
        if self.clipboard_prompt.is_some() {
            return;
        }
        self.menu = None;
        let input = cx.new(|cx| TextField::search(self.theme().accent, window, cx));
        let events = cx.subscribe_in(&input, window, |view, _, event, window, cx| match event {
            TextFieldEvent::Confirm(_) => view.run_selected(window, cx),
            TextFieldEvent::Cancel => view.close_palette(window, cx),
        });
        let changes = cx.observe(&input, |view, _, cx| {
            if let Some(palette) = view.palette.as_mut() {
                palette.selected = 0;
                palette.scroll.scroll_to_item(0);
            }
            cx.notify()
        });
        self.palette = Some(Palette {
            input,
            selected: 0,
            scroll: ScrollHandle::new(),
            _subscriptions: [events, changes],
        });
        // Drawn by GPUI over the terminals, which native surfaces would cover.
        if !self.frozen {
            self.set_frozen(true, cx);
        }
        cx.notify();
    }

    /// `sync` closes the palette, thaws the terminals and gives the active pane focus.
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.sync(window, cx);
    }

    /// Actions in the Keyboard shortcuts order, then Settings sections, then tabs; with a
    /// query, the matches best first.
    fn palette_items(&self, query: &str, cx: &Context<Self>) -> Vec<Item> {
        let (bindings, _) = keymap::resolve(&self.settings.keybinds);
        let mut by_action = keymap::by_action(&bindings);
        let mut actions: Vec<_> = ACTIONS
            .iter()
            .filter(|action| action.name != "toggle_command_palette")
            .collect();
        actions.sort_by_key(|action| action.description);
        let actions = actions.into_iter().map(|action| Item {
            label: action.description.into(),
            detail: None,
            triggers: by_action.remove(action.name).unwrap_or_default(),
            command: Command::Action(action.name),
            matched: Vec::new(),
        });
        let sections = Section::ALL.into_iter().map(|section| Item {
            label: format!("Settings: {}", section.label()),
            detail: None,
            triggers: Vec::new(),
            command: Command::Settings(section),
            matched: Vec::new(),
        });
        let tabs = self.model.tabs.iter().enumerate().map(|(index, tab)| Item {
            label: format!("Go to tab: {}", self.tab_title(tab, cx)),
            detail: self.tab_subtitle(tab, cx),
            triggers: Vec::new(),
            command: Command::Tab(index),
            matched: Vec::new(),
        });
        let items = actions.chain(sections).chain(tabs);
        if query.trim().is_empty() {
            return items.collect();
        }
        let mut scored: Vec<(i32, Item)> = items
            .filter_map(|mut item| {
                let (score, matched) = fuzzy_match(query, &item.label)?;
                item.matched = matched;
                Some((score, item))
            })
            .collect();
        scored.sort_by_key(|(score, _)| -score);
        scored.into_iter().map(|(_, item)| item).collect()
    }

    fn move_palette(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let query = palette.input.read(cx).text().to_owned();
        let count = self.palette_items(&query, cx).len();
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        if count == 0 {
            return;
        }
        palette.selected = (palette.selected as isize + step).rem_euclid(count as isize) as usize;
        palette.scroll.scroll_to_item(palette.selected);
        cx.notify();
    }

    fn run_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let query = palette.input.read(cx).text().to_owned();
        let selected = palette.selected;
        match self.palette_items(&query, cx).get(selected) {
            Some(item) => self.run_command(item.command, window, cx),
            None => self.close_palette(window, cx),
        }
    }

    fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        // Focus goes back to the active pane first, so terminal actions reach it.
        self.close_palette(window, cx);
        match command {
            Command::Action(name) => {
                if let Some(action) = crate::actions::keymap_action(name) {
                    window.dispatch_action(action, cx);
                }
            }
            Command::Settings(section) => {
                self.open_settings(window, cx);
                if let Some(PaneView::Settings(pane)) = self.pane(self.model.active_pane()) {
                    pane.update(cx, |pane, cx| pane.show_section(section, cx));
                }
            }
            Command::Tab(index) => self.select_tab(index, window, cx),
        }
    }

    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let palette = self.palette.as_ref()?;
        let theme = self.theme();
        let query = palette.input.read(cx).text().to_owned();
        let items = self.palette_items(&query, cx);
        let selected = palette.selected.min(items.len().saturating_sub(1));
        let rows = items.into_iter().enumerate().map(|(index, item)| {
            let chosen = index == selected;
            let highlight = HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                color: (!chosen).then(|| hsla(theme.accent)),
                ..Default::default()
            };
            let ranges: Vec<_> = item
                .matched
                .iter()
                .map(|&at| {
                    let len = item.label[at..].chars().next().map_or(1, char::len_utf8);
                    (at..at + len, highlight)
                })
                .collect();
            let command = item.command;
            div()
                .id(("palette-row", index))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(32.0))
                .rounded_md()
                .when(chosen, |this| {
                    this.bg(hsla(theme.accent)).text_color(hsla(theme.chrome))
                })
                .when(!chosen, |this| {
                    this.hover(|style| style.bg(hsla(theme.tab_hover)))
                })
                .on_click(
                    cx.listener(move |view, _, window, cx| view.run_command(command, window, cx)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .child(StyledText::new(item.label).with_highlights(ranges)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .when(!chosen, |this| this.text_color(hsla(theme.text_muted)))
                        .children(item.detail),
                )
                .child(
                    div().flex().gap_4().children(
                        item.triggers
                            .iter()
                            .map(|trigger| settings_pane::caps(theme, trigger, false)),
                    ),
                )
        });
        let rows: Vec<_> = rows.collect();
        let empty = rows.is_empty();
        Some(
            self.dialog(
                WIDTH,
                app::TITLE_BAR_HEIGHT + 6.0,
                div()
                    .on_mouse_down_out(
                        cx.listener(|view, _, window, cx| view.close_palette(window, cx)),
                    )
                    // The field has no use for Up and Down; they move the selection.
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                        let step = match event.keystroke.key.as_str() {
                            "up" => -1,
                            "down" => 1,
                            _ => return,
                        };
                        cx.stop_propagation();
                        view.move_palette(step, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .h(px(40.0))
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
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .when(query.is_empty(), |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .text_color(hsla(theme.text_muted))
                                                .child("Search commands, settings and tabs"),
                                        )
                                    })
                                    .child(palette.input.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id("palette-rows")
                            .max_h(px(LIST_HEIGHT))
                            .overflow_y_scroll()
                            .track_scroll(&palette.scroll)
                            .p_1()
                            .children(rows)
                            .when(empty, |this| {
                                this.child(
                                    div()
                                        .p_3()
                                        .text_color(hsla(theme.text_muted))
                                        .child("No matching commands"),
                                )
                            }),
                    ),
            ),
        )
    }
}
