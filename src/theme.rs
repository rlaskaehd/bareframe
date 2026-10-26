//! A theme is one set of colors for both the window chrome and every terminal.
use bareframe_core::settings::{Settings, ThemeName};
use gpui::{Hsla, rgb};
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
    pub terminal_background: u32,
    pub terminal_foreground: u32,
    pub palette: [u32; 16],
}

const DARK: Theme = Theme {
    chrome: 0x0f1115,
    tab_active: 0x1b1e25,
    tab_hover: 0x171a20,
    border: 0x262a33,
    text: 0xd6dbe4,
    text_muted: 0x7d8596,
    accent: 0x5aa9ff,
    terminal_background: 0x15171c,
    terminal_foreground: 0xd6dbe4,
    palette: [
        0x1f2229, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x5c6370,
        0xff7a85, 0xb5e890, 0xffd68a, 0x7cc4ff, 0xde9bf0, 0x7fd4df, 0xe6e9ef,
    ],
};

const LIGHT: Theme = Theme {
    chrome: 0xeceef2,
    tab_active: 0xfafafa,
    tab_hover: 0xe3e6eb,
    border: 0xd4d8df,
    text: 0x2f323a,
    text_muted: 0x6b7280,
    accent: 0x2f6fe4,
    terminal_background: 0xfafafa,
    terminal_foreground: 0x383a42,
    palette: [
        0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xa0a1a7, 0x696c77,
        0xf0705f, 0x6cbf5c, 0xd4a017, 0x5a8ff7, 0xc04fc0, 0x22a3d0, 0xffffff,
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
        theme.font_family = settings.font_family.clone();
        theme.font_size = Some(settings.font_size);
        theme
    }
}

pub fn hsla(value: u32) -> Hsla {
    rgb(value).into()
}

fn color(value: u32) -> TerminalColor {
    let [_, r, g, b] = value.to_be_bytes();
    TerminalColor::new(r, g, b)
}
