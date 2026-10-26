//! The Settings tab, as Warp opens it: section navigation and search on the left, the
//! section's settings on the right. Rows are built from `bareframe_core::settings::DEFS`.
use crate::{
    rename::{RenameEvent, RenameInput},
    theme::{self, Theme, hsla},
};
use bareframe_core::settings::{self, DEFS, Def, Kind, Settings};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Subscription, Window, div, prelude::*,
    px,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Section {
    Appearance,
    Features,
    KeyboardShortcuts,
    About,
}

impl Section {
    const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Features,
        Self::KeyboardShortcuts,
        Self::About,
    ];

    fn label(self) -> &'static str {
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
}

pub struct SettingsPane {
    focus: FocusHandle,
    section: Section,
    theme: &'static Theme,
    settings: Settings,
    search: Entity<RenameInput>,
    /// The text setting being edited and its field.
    editing: Option<(&'static str, Entity<RenameInput>, Subscription)>,
    _search: Subscription,
}

impl EventEmitter<SettingsEvent> for SettingsPane {}

impl SettingsPane {
    pub fn new(
        theme: &'static Theme,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| RenameInput::search(theme.accent, window, cx));
        let observed = cx.observe(&search, |_, _, cx| cx.notify());
        Self {
            focus: cx.focus_handle(),
            section: Section::Appearance,
            theme,
            settings,
            search,
            editing: None,
            _search: observed,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
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
        let input = cx.new(|cx| RenameInput::new(&value, accent, window, cx));
        let subscription = cx.subscribe(&input, move |this, _, event: &RenameEvent, cx| {
            this.editing = None;
            if let RenameEvent::Confirm(text) = event {
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
            Kind::Text => match &self.editing {
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
            (false, Section::KeyboardShortcuts) => vec![
                div()
                    .pt_4()
                    .text_color(hsla(theme.text_muted))
                    .child("Keyboard shortcuts are configured with keybind lines in the settings file for now.")
                    .into_any_element(),
            ],
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
