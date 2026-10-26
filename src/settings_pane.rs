//! The Settings tab, as Warp opens it: a pane with section navigation on the left.
use crate::theme::{self, Theme, hsla};
use gpui::{Context, FocusHandle, Window, div, prelude::*, px};

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
}

pub struct SettingsPane {
    focus: FocusHandle,
    section: Section,
    theme: &'static Theme,
}

impl SettingsPane {
    pub fn new(theme: &'static Theme, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            section: Section::Appearance,
            theme,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}

impl Render for SettingsPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let nav = Section::ALL.into_iter().map(|section| {
            let selected = section == self.section;
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
        });
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
                    .children(nav),
            )
            .child(
                div().flex_1().min_w_0().p(px(32.0)).child(
                    div()
                        .truncate()
                        .text_size(px(22.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(self.section.label()),
                ),
            )
    }
}
