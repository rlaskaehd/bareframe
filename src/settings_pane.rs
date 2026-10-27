//! The Settings tab, as Warp opens it: section navigation and search on the left, the
//! section's settings on the right. Rows are built from `bareframe_core::settings::DEFS`.
use crate::{
    text_field::{TextField, TextFieldEvent},
    theme::{self, Theme, hsla},
    widgets,
};
use bareframe_core::{
    keymap::{self, ACTIONS, Trigger},
    settings::{self, DEFS, Def, Kind, Settings},
};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Subscription, Window, div, prelude::*,
    px,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq)]
pub enum Section {
    Appearance,
    Features,
    KeyboardShortcuts,
    About,
}

impl Section {
    pub const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Features,
        Self::KeyboardShortcuts,
        Self::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Features => "Features",
            Self::KeyboardShortcuts => "Keyboard shortcuts",
            Self::About => "About",
        }
    }

    fn settings(self) -> Option<settings::Section> {
        match self {
            Self::Appearance => Some(settings::Section::Appearance),
            Self::Features => Some(settings::Section::Features),
            Self::KeyboardShortcuts | Self::About => None,
        }
    }
}

pub enum SettingsEvent {
    /// A setting was changed on screen; the workspace validates, saves and applies it.
    Set(&'static str, String),
    OpenFile,
    /// A keyboard shortcut edit: `None` restores the default, `Some(None)` removes it.
    Bind(&'static str, Option<Option<Trigger>>),
}

/// A shortcut being recorded, as Warp's "Press new keyboard shortcut" row.
struct Recording {
    action: &'static str,
    trigger: Option<Trigger>,
    /// Keeps the key interceptor alive; dropping it ends recording.
    _keys: Subscription,
}

pub struct SettingsPane {
    focus: FocusHandle,
    section: Section,
    theme: &'static Theme,
    settings: Settings,
    search: Entity<TextField>,
    /// The text setting being edited and its field.
    editing: Option<(&'static str, Entity<TextField>, Subscription)>,
    shortcut_search: Entity<TextField>,
    recording: Option<Recording>,
    _search: [Subscription; 2],
}

impl EventEmitter<SettingsEvent> for SettingsPane {}

impl SettingsPane {
    pub fn new(
        theme: &'static Theme,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextField::search(theme.accent, window, cx));
        let observed = cx.observe(&search, |_, _, cx| cx.notify());
        let shortcut_search = cx.new(|cx| TextField::search(theme.accent, window, cx));
        let shortcuts_observed = cx.observe(&shortcut_search, |_, _, cx| cx.notify());
        Self {
            focus: cx.focus_handle(),
            section: Section::Appearance,
            theme,
            settings,
            search,
            editing: None,
            shortcut_search,
            recording: None,
            _search: [observed, shortcuts_observed],
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn show_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn set_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        self.settings = settings;
        cx.notify();
    }

    fn edit(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.settings.get(key).to_owned();
        let accent = self.theme.accent;
        let input = cx.new(|cx| TextField::new(&value, accent, window, cx));
        let subscription = cx.subscribe(&input, move |this, _, event: &TextFieldEvent, cx| {
            this.editing = None;
            if let TextFieldEvent::Confirm(text) = event {
                cx.emit(SettingsEvent::Set(key, text.clone()));
            }
            cx.notify();
        });
        self.editing = Some((key, input, subscription));
        cx.notify();
    }

    fn render_control(&self, def: &'static Def, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let value = self.settings.get(def.key).to_owned();
        let key = def.key;
        match def.kind {
            Kind::Toggle => {
                let on = value == "true";
                div()
                    .id(key)
                    .flex_none()
                    .w(px(34.0))
                    .h(px(20.0))
                    .p(px(2.0))
                    .rounded_full()
                    .flex()
                    .when(on, |this| this.justify_end())
                    .bg(hsla(if on { theme.accent } else { theme.border }))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SettingsEvent::Set(key, (!on).to_string()))
                    }))
                    .child(div().size(px(16.0)).rounded_full().bg(gpui::white()))
                    .into_any_element()
            }
            Kind::Choice(options) => div()
                .flex_none()
                .flex()
                .p(px(2.0))
                .gap(px(2.0))
                .rounded_md()
                .bg(hsla(theme.tab_active))
                .children(options.iter().map(|(stored, label)| {
                    let selected = *stored == value;
                    div()
                        .id(gpui::ElementId::Name(format!("{key}:{stored}").into()))
                        .px_2()
                        .py(px(3.0))
                        .rounded_sm()
                        .text_size(px(12.0))
                        .text_color(hsla(if selected { theme.chrome } else { theme.text }))
                        .when(selected, |this| this.bg(hsla(theme.accent)))
                        .when(!selected, |this| {
                            this.hover(|style| style.bg(hsla(theme.tab_hover)))
                        })
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(SettingsEvent::Set(key, (*stored).to_owned()))
                        }))
                        .child(*label)
                }))
                .into_any_element(),
            Kind::Number { min, max, step } => {
                let number: f32 = value.parse().unwrap_or(min);
                let stepper = |id: &'static str, label: &'static str, next: f32, enabled: bool| {
                    div()
                        .id(gpui::ElementId::Name(format!("{key}:{id}").into()))
                        .size(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .text_color(hsla(if enabled {
                            theme.text
                        } else {
                            theme.text_muted
                        }))
                        .when(enabled, |this| {
                            this.hover(|style| style.bg(hsla(theme.tab_hover)))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(SettingsEvent::Set(key, next.to_string()))
                                }))
                        })
                        .child(label)
                };
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .bg(hsla(theme.tab_active))
                    .child(stepper(
                        "minus",
                        "−",
                        (number - step).max(min),
                        number > min,
                    ))
                    .child(
                        div()
                            .min_w(px(36.0))
                            .flex()
                            .justify_center()
                            .text_size(px(12.5))
                            .child(value),
                    )
                    .child(stepper("plus", "+", (number + step).min(max), number < max))
                    .into_any_element()
            }
            Kind::Text | Kind::Integer { .. } => match &self.editing {
                Some((editing, input, _)) if *editing == key => div()
                    .w(px(240.0))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(hsla(theme.accent))
                    .bg(hsla(theme.terminal_background))
                    .child(input.clone())
                    .into_any_element(),
                _ => div()
                    .id(key)
                    .w(px(240.0))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(hsla(theme.border))
                    .truncate()
                    .cursor_text()
                    .text_color(hsla(if value.is_empty() {
                        theme.text_muted
                    } else {
                        theme.text
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.edit(key, window, cx)))
                    .child(if value.is_empty() {
                        "Default".to_owned()
                    } else {
                        value
                    })
                    .into_any_element(),
            },
        }
    }

    fn render_row(&self, def: &'static Def, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        div()
            .flex()
            .items_center()
            .gap_4()
            .py(px(10.0))
            .border_b_1()
            .border_color(hsla(theme.border))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(def.label)
                    .when(!def.description.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(12.0))
                                .text_color(hsla(theme.text_muted))
                                .child(def.description),
                        )
                    }),
            )
            .child(self.render_control(def, cx))
            .into_any_element()
    }

    /// The section's settings by group, or every matching setting while searching.
    fn render_settings(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let visible: Vec<&'static Def> = DEFS
            .iter()
            .filter(|def| {
                if query.is_empty() {
                    self.section.settings() == Some(def.section)
                } else {
                    [def.label, def.description, def.group, def.key]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&query))
                }
            })
            .collect();
        if visible.is_empty() {
            return vec![
                div()
                    .text_color(hsla(self.theme.text_muted))
                    .child("No matching settings")
                    .into_any_element(),
            ];
        }
        let mut out = Vec::new();
        let mut group = "";
        for def in visible {
            if def.group != group {
                group = def.group;
                out.push(
                    div()
                        .pt_5()
                        .pb_1()
                        .text_size(px(15.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(group)
                        .into_any_element(),
                );
            }
            out.push(self.render_row(def, cx));
        }
        out
    }

    /// Starts recording a shortcut for `action`. Key presses are intercepted before any
    /// binding runs, so pressing an existing shortcut records it instead of running it.
    fn record(&mut self, action: &'static str, cx: &mut Context<Self>) {
        let pane = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let stroke = &event.keystroke;
            let modifiers = stroke.modifiers;
            let trigger = Trigger::from_keystroke(
                modifiers.control,
                modifiers.alt,
                modifiers.shift,
                modifiers.platform,
                &stroke.key,
            );
            let escape = stroke.key == "escape" && !modifiers.modified();
            let recorded = pane.update(cx, |pane, cx| {
                // Leaving Settings (another tab, a click in a terminal) ends recording, so
                // keys reach wherever focus went.
                if escape || !pane.focus.contains_focused(window, cx) {
                    pane.recording = None;
                } else if let (Some(recording), Some(trigger)) = (&mut pane.recording, trigger) {
                    recording.trigger = Some(trigger);
                }
                cx.notify();
                escape || pane.recording.is_some()
            });
            if recorded.unwrap_or(false) {
                cx.stop_propagation();
            }
        });
        self.recording = Some(Recording {
            action,
            trigger: None,
            _keys: keys,
        });
        cx.notify();
    }

    fn finish_recording(&mut self, change: Option<Option<Trigger>>, cx: &mut Context<Self>) {
        if let Some(recording) = self.recording.take() {
            cx.emit(SettingsEvent::Bind(recording.action, change));
        }
        cx.notify();
    }

    fn render_shortcuts(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme;
        let (bindings, _) = keymap::resolve(&self.settings.keybinds);
        let by_action = keymap::by_action(&bindings);
        let query = self.shortcut_search.read(cx).text().trim().to_lowercase();
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let mut actions: Vec<_> = ACTIONS.iter().collect();
        actions.sort_by_key(|action| action.description);
        let empty = query.is_empty();
        let mut out = vec![
            div()
                .pt_2()
                .text_color(hsla(theme.text_muted))
                .child("Add your own custom keybindings to existing actions. Defaults follow Ghostty; changes are saved as keybind lines in the settings file.")
                .into_any_element(),
            div()
                .id("shortcut-search")
                .relative()
                .my_3()
                .px_2()
                .h(px(30.0))
                .flex()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(hsla(theme.border))
                .overflow_hidden()
                .cursor_text()
                .on_click(cx.listener(|pane, _, window, cx| {
                    pane.shortcut_search
                        .update(cx, |input, cx| input.focus(window, cx))
                }))
                .when(empty, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(9.0))
                            .text_color(hsla(theme.text_muted))
                            .child("Search by name or by keys (ex. \"cmd d\")"),
                    )
                })
                .child(self.shortcut_search.clone())
                .into_any_element(),
        ];
        for action in actions {
            let triggers = by_action.get(action.name).cloned().unwrap_or_default();
            let matches = tokens.is_empty()
                || action.description.to_lowercase().contains(&query)
                || triggers.iter().any(|trigger| {
                    let text = trigger.to_string();
                    tokens.iter().all(|token| text.contains(token))
                });
            if !matches {
                continue;
            }
            out.push(self.render_shortcut(action, &triggers, &bindings, cx));
        }
        out
    }

    fn render_shortcut(
        &self,
        action: &'static keymap::Action,
        triggers: &[Trigger],
        bindings: &BTreeMap<Trigger, &'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let name = action.name;
        let editing = self
            .recording
            .as_ref()
            .filter(|recording| recording.action == name);
        let row = div()
            .id(gpui::ElementId::Name(format!("shortcut:{name}").into()))
            .flex()
            .items_center()
            .gap_4()
            .px_3()
            .py(px(9.0))
            .rounded_md()
            .when(editing.is_some(), |this| {
                this.bg(hsla(theme.accent)).text_color(hsla(theme.chrome))
            })
            .when(editing.is_none(), |this| {
                this.hover(|style| style.bg(hsla(theme.tab_hover)))
                    .on_click(cx.listener(move |pane, _, _, cx| pane.record(name, cx)))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(action.description),
            )
            .child(
                div()
                    .flex()
                    // Wider than the gap between caps, so each shortcut reads as one group.
                    .gap_4()
                    .children(
                        triggers
                            .iter()
                            .map(|trigger| widgets::caps(theme, trigger, false)),
                    ),
            );
        let Some(recording) = editing else {
            return row.into_any_element();
        };
        let conflict = recording.trigger.as_ref().and_then(|trigger| {
            bindings
                .get(trigger)
                .filter(|other| **other != name)
                .and_then(|other| keymap::find(other))
        });
        let recorded = recording.trigger.clone();
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(gpui::ElementId::Name(format!("{name}:{id}").into()))
                .px_2()
                .py(px(3.0))
                .rounded_sm()
                .border_1()
                .border_color(hsla(theme.text_muted))
                .text_size(px(12.0))
                .hover(|style| style.bg(hsla(theme.tab_hover)))
                .child(label)
        };
        div()
            .flex()
            .flex_col()
            .child(row)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(hsla(theme.tab_active))
                    .child(match &recording.trigger {
                        Some(trigger) => div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(widgets::caps(theme, trigger, conflict.is_some()))
                            .when_some(conflict, |this, other| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(hsla(theme.group_colors[3]))
                                        .child(format!(
                                            "Used by “{}”; saving moves it here",
                                            other.description
                                        )),
                                )
                            })
                            .into_any_element(),
                        None => div()
                            .text_color(hsla(theme.text_muted))
                            .child("Press new keyboard shortcut (Esc cancels)")
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("clear", "Clear").on_click(
                                cx.listener(|pane, _, _, cx| pane.finish_recording(Some(None), cx)),
                            ))
                            .child(button("default", "Default").on_click(
                                cx.listener(|pane, _, _, cx| pane.finish_recording(None, cx)),
                            ))
                            .child(button("cancel", "Cancel").on_click(cx.listener(
                                |pane, _, _, cx| {
                                    pane.recording = None;
                                    cx.notify()
                                },
                            )))
                            .when_some(recorded, |this, trigger| {
                                this.child(button("save", "Save").on_click(cx.listener(
                                    move |pane, _, _, cx| {
                                        pane.finish_recording(Some(Some(trigger.clone())), cx)
                                    },
                                )))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_about(&self) -> AnyElement {
        let theme = self.theme;
        let line = |text: String| div().py_1().text_color(hsla(theme.text_muted)).child(text);
        div()
            .pt_4()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("bareframe {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(line("A modular terminal workspace built with GPUI and Ghostty.".into()))
            .child(line(
                "Licensed under the GNU Affero General Public License v3.0 or later.".into(),
            ))
            .child(line(
                "Bundles Ghostty's zsh shell integration (GPL-3.0); libghostty and gpui-libghostty are MIT, GPUI is Apache-2.0.".into(),
            ))
            .into_any_element()
    }
}

impl Render for SettingsPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let searching = !self.search.read(cx).text().trim().is_empty();
        let nav = Section::ALL
            .into_iter()
            .map(|section| {
                let selected = !searching && section == self.section;
                div()
                    .id(section.label())
                    .px_3()
                    .py(px(6.0))
                    .rounded_md()
                    .text_color(hsla(if selected { theme.chrome } else { theme.text }))
                    .when(selected, |this| this.bg(hsla(theme.accent)))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(hsla(theme.tab_hover)))
                    })
                    .on_click(cx.listener(move |pane, _, _, cx| {
                        pane.section = section;
                        cx.notify();
                    }))
                    .child(section.label())
            })
            .collect::<Vec<_>>();
        let title = if searching {
            "Search results"
        } else {
            self.section.label()
        };
        let body: Vec<AnyElement> = match (searching, self.section) {
            (false, Section::About) => vec![self.render_about()],
            (false, Section::KeyboardShortcuts) => self.render_shortcuts(cx),
            _ => self.render_settings(cx),
        };
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(hsla(theme.terminal_background))
            .font_family(theme::UI_FONT)
            .text_size(px(13.0))
            .text_color(hsla(theme.text))
            .child(
                div()
                    .w(px(220.0))
                    .flex_shrink_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .p_3()
                    .border_r_1()
                    .border_color(hsla(theme.border))
                    .child(
                        div()
                            .id("settings-search")
                            .relative()
                            .mb_2()
                            .px_2()
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(hsla(theme.border))
                            .overflow_hidden()
                            .cursor_text()
                            .on_click(cx.listener(|pane, _, window, cx| {
                                pane.search.update(cx, |input, cx| input.focus(window, cx))
                            }))
                            .when(!searching, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left(px(9.0))
                                        .text_color(hsla(theme.text_muted))
                                        .child("Search"),
                                )
                            })
                            .child(self.search.clone()),
                    )
                    .children(nav)
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("open-settings-file")
                            .px_3()
                            .py(px(6.0))
                            .rounded_md()
                            .border_1()
                            .border_color(hsla(theme.border))
                            .flex()
                            .justify_center()
                            .hover(|style| style.bg(hsla(theme.tab_hover)))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile)))
                            .child("Open settings file"),
                    ),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .px(px(32.0))
                    .py(px(24.0))
                    .child(
                        div()
                            .max_w(px(760.0))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(22.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .children(body),
                    ),
            )
    }
}
