//! Settings read from `~/.config/bareframe/config` (`key = value`, like Ghostty).
//!
//! Every setting is declared once in [`DEFS`]: the Settings screen renders from it, the
//! parser validates against it, and terminal settings name the Ghostty keys they become.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
}

pub const DEFAULT_FONT_SIZE: f32 = 13.0;
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 6.0..=72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Features,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Toggle,
    /// Stored value and label.
    Choice(&'static [(&'static str, &'static str)]),
    Number {
        min: f32,
        max: f32,
        step: f32,
    },
    /// Free text on one line; empty means "Ghostty's or the system's default".
    Text,
    /// A whole number typed in, applied on Enter or when the field loses focus.
    Integer {
        min: i64,
        max: i64,
    },
}

/// How a setting reaches the terminals.
#[derive(Clone, Copy, Debug)]
pub enum Ghostty {
    /// Read by bareframe itself.
    App,
    /// Passed through under the same key.
    Same,
    /// Passed through under another key; `None` value means "leave Ghostty's default".
    Map(fn(&str) -> Vec<(&'static str, String)>),
}

#[derive(Clone, Copy, Debug)]
pub struct Def {
    pub key: &'static str,
    pub section: Section,
    pub group: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: Kind,
    pub default: &'static str,
    pub ghostty: Ghostty,
}

// One row of the table below; positional so each entry reads like a table line.
#[allow(clippy::too_many_arguments)]
const fn def(
    section: Section,
    group: &'static str,
    key: &'static str,
    label: &'static str,
    description: &'static str,
    kind: Kind,
    default: &'static str,
    ghostty: Ghostty,
) -> Def {
    Def {
        key,
        section,
        group,
        label,
        description,
        kind,
        default,
        ghostty,
    }
}

use Ghostty::{App, Map, Same};
use Kind::{Choice, Integer, Number, Text, Toggle};
use Section::{Appearance, Features};

pub const DEFS: &[Def] = &[
    def(
        Appearance,
        "Themes",
        "theme",
        "Theme",
        "Sync with OS follows the system's light or dark appearance.",
        Choice(&[
            ("dark", "Dark"),
            ("light", "Light"),
            ("system", "Sync with OS"),
        ]),
        "dark",
        App,
    ),
    def(
        Appearance,
        "Text",
        "font-family",
        "Terminal font",
        "A font installed on this Mac. Empty uses Ghostty's built-in font.",
        Text,
        "",
        App,
    ),
    def(
        Appearance,
        "Text",
        "font-size",
        "Font size (pt)",
        "",
        Number {
            min: 6.0,
            max: 72.0,
            step: 1.0,
        },
        "13",
        App,
    ),
    def(
        Appearance,
        "Text",
        "font-style",
        "Font weight",
        "Uses the font's style of that name when it has one.",
        Choice(&[
            ("default", "Normal"),
            ("Light", "Light"),
            ("Medium", "Medium"),
            ("Semibold", "Semibold"),
            ("Bold", "Bold"),
        ]),
        "default",
        Map(|value| match value {
            "default" => Vec::new(),
            style => vec![("font-style", style.to_owned())],
        }),
    ),
    def(
        Appearance,
        "Text",
        "adjust-cell-height",
        "Line height adjustment (%)",
        "Extra space added to each line.",
        Number {
            min: -20.0,
            max: 100.0,
            step: 5.0,
        },
        "0",
        Map(|value| vec![("adjust-cell-height", format!("{value}%"))]),
    ),
    def(
        Appearance,
        "Text",
        "font-thicken",
        "Thicken font strokes",
        "Draws text with heavier strokes, like macOS font smoothing.",
        Toggle,
        "false",
        Same,
    ),
    def(
        Appearance,
        "Text",
        "minimum-contrast",
        "Minimum contrast",
        "Raises text contrast against its background up to this ratio (1 = off, 21 = black on white).",
        Number {
            min: 1.0,
            max: 21.0,
            step: 0.5,
        },
        "1",
        Same,
    ),
    def(
        Appearance,
        "Text",
        "font-ligatures",
        "Show ligatures",
        "Joins character sequences such as -> into a single glyph when the font has them.",
        Toggle,
        "true",
        Map(|value| match value {
            "false" => ["-calt", "-liga", "-dlig"]
                .into_iter()
                .map(|feature| ("font-feature", feature.to_owned()))
                .collect(),
            _ => Vec::new(),
        }),
    ),
    def(
        Appearance,
        "Cursor",
        "cursor-style",
        "Cursor type",
        "",
        Choice(&[
            ("block", "Block"),
            ("bar", "Bar"),
            ("underline", "Underline"),
        ]),
        "block",
        Same,
    ),
    def(
        Appearance,
        "Cursor",
        "cursor-style-blink",
        "Blinking cursor",
        "",
        Toggle,
        "true",
        Same,
    ),
    def(
        Appearance,
        "Panes",
        "terminal-padding",
        "Terminal padding (px)",
        "Space between the terminal text and the pane edge.",
        Number {
            min: 0.0,
            max: 40.0,
            step: 2.0,
        },
        "2",
        Map(|value| {
            vec![
                ("window-padding-x", value.to_owned()),
                ("window-padding-y", value.to_owned()),
            ]
        }),
    ),
    def(
        Appearance,
        "Tabs",
        "preserve-tab-color",
        "Preserve active tab color for new tabs",
        "A new tab takes the color of the tab it was opened from.",
        Toggle,
        "false",
        App,
    ),
    def(
        Appearance,
        "Tools panel",
        "project-explorer",
        "Project explorer",
        "Show the project explorer / file tree tab in the tools panel.",
        Toggle,
        "true",
        App,
    ),
    def(
        Appearance,
        "Tools panel",
        "global-search",
        "Global search",
        "Show the global file search tab in the tools panel.",
        Toggle,
        "true",
        App,
    ),
    def(
        Appearance,
        "Tools panel",
        "show-hidden-files",
        "Show hidden files",
        "Show files and folders whose names start with a dot in the project explorer.",
        Toggle,
        "true",
        App,
    ),
    def(
        Features,
        "General",
        "restore-state",
        "Restore windows, tabs, and panes on startup",
        "Terminals reopen as new shells in their last directories.",
        Toggle,
        "true",
        App,
    ),
    def(
        Features,
        "General",
        "mouse-scroll-multiplier",
        "Mouse wheel scroll speed",
        "Multiplies the lines scrolled by each wheel step.",
        Number {
            min: 0.5,
            max: 10.0,
            step: 0.5,
        },
        "1",
        Same,
    ),
    def(
        Features,
        "Session",
        "shell",
        "Default shell for new sessions",
        "Path to a shell such as /bin/zsh. Empty uses the login shell.",
        Text,
        "",
        App,
    ),
    def(
        Features,
        "Session",
        "working-directory",
        "Working directory for new sessions",
        "",
        Choice(&[
            ("previous", "Previous session's directory"),
            ("home", "Home directory"),
        ]),
        "previous",
        App,
    ),
    def(
        Features,
        "Session",
        "new-tab-position",
        "New tab placement",
        "",
        Choice(&[
            ("after-current", "After current tab"),
            ("end", "After all tabs"),
        ]),
        "after-current",
        App,
    ),
    def(
        Features,
        "Keys",
        "left-option-as-meta",
        "Left Option key is Meta",
        "Sends Esc-prefixed keys instead of special characters.",
        Toggle,
        "false",
        App,
    ),
    def(
        Features,
        "Keys",
        "right-option-as-meta",
        "Right Option key is Meta",
        "",
        Toggle,
        "false",
        App,
    ),
    def(
        Features,
        "Terminal",
        "mouse-reporting",
        "Enable mouse reporting",
        "Lets programs such as vim and htop receive clicks and scrolling.",
        Toggle,
        "true",
        Same,
    ),
    def(
        Features,
        "Terminal",
        "scrollback-limit-lines",
        "Scrollback limit (lines)",
        "Lines each terminal keeps above the screen; -1 for no line limit. Ghostty also caps \
         it at 50 MB of memory per terminal. Applies to terminals opened afterwards.",
        Integer {
            min: -1,
            max: 100_000_000,
        },
        "4096",
        Map(|value| {
            let lines = if value == "-1" { "unlimited" } else { value };
            vec![("scrollback-limit-lines", lines.to_owned())]
        }),
    ),
    def(
        Features,
        "Terminal",
        "copy-on-select",
        "Copy on select",
        "Copies selected text to the clipboard right away.",
        Toggle,
        "true",
        Same,
    ),
    def(
        Features,
        "Terminal",
        "selection-word-chars",
        "Characters that end a word on double-click",
        "Empty uses Ghostty's default set.",
        Text,
        "",
        Map(|value| match value {
            "" => Vec::new(),
            chars => vec![("selection-word-chars", chars.to_owned())],
        }),
    ),
    def(
        Features,
        "Terminal",
        "clipboard-access",
        "Clipboard access (OSC 52)",
        "Controls whether programs running in the terminal can read or write your clipboard.",
        Choice(&[("deny", "Deny"), ("ask", "Ask"), ("allow", "Allow")]),
        "deny",
        Map(|value| {
            vec![
                ("clipboard-read", value.to_owned()),
                ("clipboard-write", value.to_owned()),
            ]
        }),
    ),
    def(
        Features,
        "Code review",
        "code-review-button",
        "Show the code review button",
        "Show the diff button in the top bar, with +N −M when the active pane's repository has changes.",
        Toggle,
        "true",
        App,
    ),
    def(
        Features,
        "Files",
        "open-markdown-in-viewer",
        "Open Markdown files in the Markdown viewer",
        "Open .md files from the project explorer and global search in bareframe instead of the default app.",
        Toggle,
        "true",
        App,
    ),
    def(
        Features,
        "Files",
        "open-files-in-editor",
        "Open text files in the code editor",
        "Open code and text files from the project explorer, global search and code review in bareframe's editor instead of the default app.",
        Toggle,
        "true",
        App,
    ),
    def(
        Features,
        "Editor",
        "editor-line-numbers",
        "Line numbers",
        "Show each line's number, its distance from the cursor's line, or none.",
        Choice(&[
            ("absolute", "Absolute"),
            ("relative", "Relative"),
            ("off", "Off"),
        ]),
        "absolute",
        App,
    ),
    def(
        Features,
        "Editor",
        "group-editor-files",
        "Group files into single editor pane",
        "Open files in the active tab's editor pane as file tabs instead of new panes.",
        Toggle,
        "true",
        App,
    ),
    def(
        Features,
        "Editor",
        "editor-autosave",
        "Save files automatically",
        "Save a file in the editor a second after the last change.",
        Toggle,
        "false",
        App,
    ),
    def(
        Features,
        "Files",
        "open-file-layout",
        "Choose a layout to open files",
        "Where files opened in bareframe appear.",
        Choice(&[("split", "Split pane"), ("tab", "New tab")]),
        "split",
        App,
    ),
];

pub fn find(key: &str) -> Option<&'static Def> {
    DEFS.iter().find(|def| def.key == key)
}

/// A value in the form the config file stores, or why it is not one.
pub fn normalize(def: &Def, value: &str) -> Result<String, String> {
    let value = value.trim();
    match def.kind {
        Toggle => match value.to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" => Ok("true".into()),
            "false" | "off" | "no" => Ok("false".into()),
            _ => Err(format!("{} must be true or false", def.key)),
        },
        Choice(options) => options
            .iter()
            .find(|(stored, _)| stored.eq_ignore_ascii_case(value))
            .map(|(stored, _)| (*stored).to_owned())
            .ok_or_else(|| {
                let names: Vec<&str> = options.iter().map(|(stored, _)| *stored).collect();
                format!("{} must be one of {}", def.key, names.join(", "))
            }),
        Integer { min, max } => match value.parse::<i64>() {
            Ok(number) if (min..=max).contains(&number) => Ok(number.to_string()),
            _ => Err(format!(
                "{} must be a whole number from {min} to {max}",
                def.key
            )),
        },
        Number { min, max, .. } => match value.parse::<f32>() {
            Ok(number) if (min..=max).contains(&number) => Ok(format_number(number)),
            _ => Err(format!("{} must be a number from {min} to {max}", def.key)),
        },
        // The config syntax has no quoting, so a control character could start a new key.
        Text if value.chars().any(char::is_control) => {
            Err(format!("{} must be a single line", def.key))
        }
        Text => Ok(value.to_owned()),
    }
}

fn format_number(number: f32) -> String {
    let rounded = (number * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    values: BTreeMap<&'static str, String>,
    /// `keybind = trigger=action` lines, in file order (applied by the app).
    pub keybinds: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            values: DEFS
                .iter()
                .map(|def| (def.key, def.default.to_owned()))
                .collect(),
            keybinds: Vec::new(),
        }
    }
}

impl Settings {
    /// Invalid lines keep the default and are reported, so a typo never blocks startup.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut warnings = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("line {}: expected key = value", number + 1));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if key == "keybind" {
                settings.keybinds.push(value.to_owned());
                continue;
            }
            let Some(def) = find(key) else {
                warnings.push(format!("line {}: unknown key {key:?}", number + 1));
                continue;
            };
            match normalize(def, value) {
                Ok(value) => {
                    settings.values.insert(def.key, value);
                }
                Err(error) => warnings.push(format!("line {}: {error}", number + 1)),
            }
        }
        (settings, warnings)
    }

    pub fn get(&self, key: &str) -> &str {
        self.values.get(key).map_or("", String::as_str)
    }

    pub fn flag(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    pub fn number(&self, key: &str) -> f32 {
        self.get(key).parse().unwrap_or_default()
    }

    /// Validates and stores one value; the caller writes it back to the file.
    pub fn set(&mut self, key: &str, value: &str) -> Result<String, String> {
        let def = find(key).ok_or_else(|| format!("unknown key {key:?}"))?;
        let value = normalize(def, value)?;
        self.values.insert(def.key, value.clone());
        Ok(value)
    }

    /// `None` for `system`: the app follows the window's appearance.
    pub fn theme(&self) -> Option<ThemeName> {
        match self.get("theme") {
            "light" => Some(ThemeName::Light),
            "system" => None,
            _ => Some(ThemeName::Dark),
        }
    }

    pub fn font_family(&self) -> Option<&str> {
        Some(self.get("font-family")).filter(|family| !family.is_empty())
    }

    pub fn font_size(&self) -> f32 {
        self.number("font-size")
    }

    /// Returns the size after `steps` points, clamped to the supported range.
    pub fn step_font_size(&mut self, steps: f32) -> f32 {
        let size =
            (self.font_size() + steps).clamp(*FONT_SIZE_RANGE.start(), *FONT_SIZE_RANGE.end());
        self.values.insert("font-size", format_number(size));
        size
    }

    /// Lines for the generated Ghostty config, beyond colors and font.
    pub fn ghostty_lines(&self) -> Vec<(&'static str, String)> {
        let mut lines = Vec::new();
        for def in DEFS {
            let value = self.get(def.key);
            match def.ghostty {
                App => {}
                Same => lines.push((def.key, value.to_owned())),
                Map(map) => lines.extend(map(value)),
            }
        }
        let option = match (
            self.flag("left-option-as-meta"),
            self.flag("right-option-as-meta"),
        ) {
            (true, true) => "true",
            (true, false) => "left",
            (false, true) => "right",
            (false, false) => "false",
        };
        lines.push(("macos-option-as-alt", option.to_owned()));
        lines
    }
}

/// Rewrites `key`'s line in a config file's text, keeping comments and other lines. The
/// last line for the key wins when reading, so that one is replaced and earlier duplicates
/// are dropped; a default value removes the key entirely.
pub fn write_value(text: &str, key: &str, value: &str, default: &str) -> String {
    let is_key = |line: &str| {
        line.split_once('=')
            .is_some_and(|(name, _)| name.trim() == key)
            && !line.trim_start().starts_with('#')
    };
    let last = text
        .lines()
        .enumerate()
        .filter(|(_, line)| is_key(line))
        .map(|(index, _)| index)
        .last();
    let mut lines: Vec<String> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if !is_key(line) {
            lines.push(line.to_owned());
        } else if Some(index) == last && value != default {
            lines.push(format!("{key} = {value}"));
        }
    }
    if last.is_none() && value != default {
        lines.push(format!("{key} = {value}"));
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

pub fn config_path(xdg_config_home: Option<&Path>, home: &Path) -> PathBuf {
    xdg_config_home
        .filter(|path| path.is_absolute())
        .map_or_else(|| home.join(".config"), Path::to_path_buf)
        .join("bareframe/config")
}

/// Where the window layout is saved, next to the config file.
pub fn state_path(xdg_config_home: Option<&Path>, home: &Path) -> PathBuf {
    config_path(xdg_config_home, home).with_file_name("state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_keys_apply_and_bad_lines_keep_defaults() {
        let (settings, warnings) = Settings::parse(
            "# comment\ntheme = Light\nfont-family = JetBrains Mono\nfont-size = 99\nbogus\ncolor = red\nkeybind = cmd+t=new_tab\n",
        );
        assert_eq!(settings.theme(), Some(ThemeName::Light));
        assert_eq!(settings.font_family(), Some("JetBrains Mono"));
        assert_eq!(settings.font_size(), DEFAULT_FONT_SIZE);
        assert_eq!(settings.keybinds, ["cmd+t=new_tab"]);
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].starts_with("line 4:"));
    }

    #[test]
    fn every_default_is_valid_for_its_kind() {
        for def in DEFS {
            assert_eq!(
                normalize(def, def.default).as_deref(),
                Ok(def.default),
                "{}",
                def.key
            );
        }
        let keys: std::collections::HashSet<_> = DEFS.iter().map(|def| def.key).collect();
        assert_eq!(keys.len(), DEFS.len(), "keys are unique");
    }

    #[test]
    fn values_are_validated_by_kind() {
        let mut settings = Settings::default();
        assert_eq!(settings.set("cursor-style", "BAR"), Ok("bar".into()));
        assert_eq!(settings.set("font-thicken", "on"), Ok("true".into()));
        assert_eq!(settings.set("minimum-contrast", "3.50"), Ok("3.5".into()));
        assert!(settings.set("minimum-contrast", "30").is_err());
        assert!(settings.set("cursor-style", "beam").is_err());
        assert!(
            settings
                .set("font-family", "Menlo\nbackground = red")
                .is_err()
        );
        assert!(settings.set("nope", "1").is_err());
    }

    #[test]
    fn terminal_settings_become_ghostty_lines() {
        let mut settings = Settings::default();
        settings.set("font-ligatures", "false").unwrap();
        settings.set("terminal-padding", "8").unwrap();
        settings.set("left-option-as-meta", "true").unwrap();
        assert!(has_default_scrollback(&Settings::default()));
        for bad in ["-2", "1.5", "lots", ""] {
            assert!(
                settings.set("scrollback-limit-lines", bad).is_err(),
                "{bad}"
            );
        }
        assert_eq!(
            settings.set("scrollback-limit-lines", " 10000 "),
            Ok("10000".into())
        );
        let lines = settings.ghostty_lines();
        let has = |key: &str, value: &str| lines.iter().any(|(k, v)| *k == key && v == value);
        assert!(has("font-feature", "-calt"));
        assert!(has("window-padding-y", "8"));
        assert!(has("macos-option-as-alt", "left"));
        assert!(has("adjust-cell-height", "0%"));
        assert!(has("clipboard-read", "deny"));
        assert!(has("scrollback-limit-lines", "10000"));
        assert!(
            !lines.iter().any(|(key, _)| *key == "font-style"),
            "default omitted"
        );
        assert!(
            !lines.iter().any(|(key, _)| *key == "restore-state"),
            "app setting"
        );
    }

    fn has_default_scrollback(settings: &Settings) -> bool {
        settings
            .ghostty_lines()
            .contains(&("scrollback-limit-lines", "4096".into()))
    }

    #[test]
    fn no_line_limit_is_minus_one() {
        let mut settings = Settings::default();
        settings.set("scrollback-limit-lines", "-1").unwrap();
        assert!(
            settings
                .ghostty_lines()
                .contains(&("scrollback-limit-lines", "unlimited".into()))
        );
    }

    #[test]
    fn writing_keeps_comments_and_replaces_the_last_line() {
        let text = "# mine\nfont-size = 12\ntheme = light\nfont-size = 14\n";
        assert_eq!(
            write_value(text, "font-size", "16", "13"),
            "# mine\ntheme = light\nfont-size = 16\n"
        );
        assert_eq!(
            write_value(text, "font-size", "13", "13"),
            "# mine\ntheme = light\n"
        );
        assert_eq!(
            write_value("", "cursor-style", "bar", "block"),
            "cursor-style = bar\n"
        );
        assert_eq!(
            write_value("# font-size = 9\n", "font-size", "16", "13"),
            "# font-size = 9\nfont-size = 16\n"
        );
    }

    #[test]
    fn font_size_steps_are_clamped() {
        let mut settings = Settings::default();
        assert_eq!(settings.step_font_size(1.0), 14.0);
        assert_eq!(settings.step_font_size(-100.0), 6.0);
        assert_eq!(settings.get("font-size"), "6");
    }

    #[test]
    fn config_path_prefers_absolute_xdg_home() {
        let home = Path::new("/Users/me");
        assert_eq!(
            config_path(None, home),
            Path::new("/Users/me/.config/bareframe/config")
        );
        assert_eq!(
            config_path(Some(Path::new("relative")), home),
            Path::new("/Users/me/.config/bareframe/config")
        );
        assert_eq!(
            config_path(Some(Path::new("/xdg")), home),
            Path::new("/xdg/bareframe/config")
        );
    }
}
