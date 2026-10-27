//! A theme is one set of colors for both the window chrome and every terminal.
use bareframe_core::{
    settings::{Settings, ThemeName},
    workspace::GROUP_COLORS,
};
use gpui::{Div, ElementId, Hsla, Stateful, div, prelude::*, rgb};
use gpui_libghostty::{TerminalColor, TerminalTheme};

pub const UI_FONT: &str = ".SystemUIFont";

pub struct Theme {
    pub chrome: u32,
    pub tab_active: u32,
    pub tab_hover: u32,
    pub border: u32,
    pub text: u32,
    pub text_muted: u32,
    pub accent: u32,
    /// Indexed by `Group::color`.
    pub group_colors: [u32; GROUP_COLORS],
    pub terminal_background: u32,
    pub terminal_foreground: u32,
    pub palette: [u32; 16],
}

// Both themes take their colors from the Claude app: warm grays, ivory and the clay accent.
// The values are read off the app, not an official palette.
const DARK: Theme = Theme {
    chrome: 0x1f1e1d,
    tab_active: 0x30302e,
    tab_hover: 0x2a2a28,
    border: 0x3e3d39,
    text: 0xfaf9f5,
    text_muted: 0x9c9a92,
    accent: 0xd97757,
    group_colors: [0xd97757, 0x6a9bcc, 0x8fa36f, 0xd4a27f, 0xc77d9a, 0x6fb0aa],
    terminal_background: 0x262624,
    terminal_foreground: 0xf0eee6,
    palette: [
        0x3d3d3a, 0xe5735f, 0x9fbf7f, 0xe0b765, 0x7aa6da, 0xc596c7, 0x76b8b0, 0xc2c0b6, 0x73726c,
        0xf08a77, 0xb5d495, 0xedca85, 0x96bce6, 0xd8acd9, 0x92cdc5, 0xfaf9f5,
    ],
};

const LIGHT: Theme = Theme {
    chrome: 0xf5f4ed,
    tab_active: 0xe8e6dc,
    tab_hover: 0xefede5,
    border: 0xdedcd3,
    text: 0x141413,
    text_muted: 0x73726c,
    accent: 0xc6613f,
    group_colors: [0xc6613f, 0x4f7fb0, 0x5f7a45, 0xb7834f, 0xb04f72, 0x3f8a8a],
    terminal_background: 0xfaf9f5,
    terminal_foreground: 0x1f1e1d,
    palette: [
        0x3d3d3a, 0xc0453a, 0x4f7a32, 0xa66f00, 0x3b6ea8, 0x9b4f96, 0x2f7d78, 0xb7b5ad, 0x73726c,
        0xd2573f, 0x5f8f3e, 0xbf8a1f, 0x4f86c6, 0xb366ad, 0x3c9690, 0xffffff,
    ],
};

impl Theme {
    pub fn get(name: ThemeName) -> &'static Self {
        match name {
            ThemeName::Dark => &DARK,
            ThemeName::Light => &LIGHT,
        }
    }

    pub fn terminal(&self, settings: &Settings) -> TerminalTheme {
        let mut theme = TerminalTheme::new(
            color(self.terminal_background),
            color(self.terminal_foreground),
            self.palette.map(color),
        );
        theme.font_family = settings.font_family().map(str::to_owned);
        theme.font_size = Some(settings.font_size());
        theme.extra = settings
            .ghostty_lines()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect();
        theme
    }
}

/// A bordered button; `primary` fills it with the accent. Callers set the padding.
pub fn button(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: &'static str,
    primary: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .rounded_md()
        .border_1()
        .border_color(hsla(if primary { theme.accent } else { theme.border }))
        .when(primary, |this| {
            this.bg(hsla(theme.accent)).text_color(hsla(theme.chrome))
        })
        .hover(|style| style.opacity(0.85))
        .child(label)
}

/// A plain-text button in the muted color that lights up under the pointer. Callers set
/// the padding and corners.
pub fn ghost_button(theme: &Theme, id: impl Into<ElementId>, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .text_color(hsla(theme.text_muted))
        .hover(|style| style.bg(hsla(theme.tab_hover)).text_color(hsla(theme.text)))
        .child(label)
}

pub fn hsla(value: u32) -> Hsla {
    rgb(value).into()
}

fn color(value: u32) -> TerminalColor {
    let [_, r, g, b] = value.to_be_bytes();
    TerminalColor::new(r, g, b)
}
