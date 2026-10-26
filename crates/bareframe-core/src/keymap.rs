//! Keyboard shortcuts: the actions bareframe binds, their Ghostty-style defaults, and the
//! `keybind = trigger=action` lines of the settings file that change them.
//!
//! Defaults follow Ghostty on macOS; bareframe's own actions (sidebar, tools panel, tab
//! groups) follow Warp. Triggers are written as in Ghostty (`cmd+shift+bracket_left`) and
//! converted to GPUI's form (`cmd-{`) by [`Trigger::to_gpui`].
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere in the window.
    Window,
    /// Only while a terminal has focus (the terminal handles the action).
    Terminal,
}

#[derive(Clone, Copy, Debug)]
pub struct Action {
    /// Ghostty's action name where Ghostty has one.
    pub name: &'static str,
    pub description: &'static str,
    pub defaults: &'static [&'static str],
    pub scope: Scope,
}

const fn action(
    name: &'static str,
    description: &'static str,
    defaults: &'static [&'static str],
    scope: Scope,
) -> Action {
    Action {
        name,
        description,
        defaults,
        scope,
    }
}

use Scope::{Terminal, Window};

pub const ACTIONS: &[Action] = &[
    action("new_tab", "New tab", &["cmd+t"], Window),
    action("close_surface", "Close pane", &["cmd+w"], Window),
    action("close_tab", "Close tab", &["cmd+alt+w"], Window),
    action("close_window", "Close window", &["cmd+shift+w"], Window),
    action("quit", "Quit bareframe", &["cmd+q"], Window),
    action("new_split:right", "Split pane right", &["cmd+d"], Window),
    action(
        "new_split:down",
        "Split pane down",
        &["cmd+shift+d"],
        Window,
    ),
    action(
        "goto_split:previous",
        "Activate previous pane",
        &["cmd+bracket_left"],
        Window,
    ),
    action(
        "goto_split:next",
        "Activate next pane",
        &["cmd+bracket_right"],
        Window,
    ),
    action(
        "goto_split:left",
        "Focus pane on the left",
        &["cmd+alt+arrow_left"],
        Window,
    ),
    action(
        "goto_split:right",
        "Focus pane on the right",
        &["cmd+alt+arrow_right"],
        Window,
    ),
    action(
        "goto_split:up",
        "Focus pane above",
        &["cmd+alt+arrow_up"],
        Window,
    ),
    action(
        "goto_split:down",
        "Focus pane below",
        &["cmd+alt+arrow_down"],
        Window,
    ),
    action(
        "equalize_splits",
        "Make panes equal size",
        &["cmd+ctrl+equal"],
        Window,
    ),
    action(
        "toggle_split_zoom",
        "Maximize pane",
        &["cmd+shift+enter"],
        Window,
    ),
    action(
        "previous_tab",
        "Activate previous tab",
        &["cmd+shift+bracket_left", "ctrl+shift+tab"],
        Window,
    ),
    action(
        "next_tab",
        "Activate next tab",
        &["cmd+shift+bracket_right", "ctrl+tab"],
        Window,
    ),
    action("goto_tab:1", "Switch to tab 1", &["cmd+digit_1"], Window),
    action("goto_tab:2", "Switch to tab 2", &["cmd+digit_2"], Window),
    action("goto_tab:3", "Switch to tab 3", &["cmd+digit_3"], Window),
    action("goto_tab:4", "Switch to tab 4", &["cmd+digit_4"], Window),
    action("goto_tab:5", "Switch to tab 5", &["cmd+digit_5"], Window),
    action("goto_tab:6", "Switch to tab 6", &["cmd+digit_6"], Window),
    action("goto_tab:7", "Switch to tab 7", &["cmd+digit_7"], Window),
    action("goto_tab:8", "Switch to tab 8", &["cmd+digit_8"], Window),
    action("last_tab", "Switch to last tab", &["cmd+digit_9"], Window),
    action("copy_to_clipboard", "Copy", &["cmd+c"], Terminal),
    action("paste_from_clipboard", "Paste", &["cmd+v"], Terminal),
    action("select_all", "Select all", &["cmd+a"], Terminal),
    action("clear_screen", "Clear screen", &["cmd+k"], Terminal),
    action("scroll_to_top", "Scroll to top", &["cmd+home"], Terminal),
    action(
        "scroll_to_bottom",
        "Scroll to bottom",
        &["cmd+end"],
        Terminal,
    ),
    action(
        "scroll_page_up",
        "Scroll page up",
        &["cmd+page_up"],
        Terminal,
    ),
    action(
        "scroll_page_down",
        "Scroll page down",
        &["cmd+page_down"],
        Terminal,
    ),
    action(
        "jump_to_prompt:-1",
        "Jump to previous prompt",
        &["cmd+arrow_up"],
        Terminal,
    ),
    action(
        "jump_to_prompt:1",
        "Jump to next prompt",
        &["cmd+arrow_down"],
        Terminal,
    ),
    action(
        "increase_font_size:1",
        "Increase font size",
        &["cmd+equal", "cmd+plus"],
        Window,
    ),
    action(
        "decrease_font_size:1",
        "Decrease font size",
        &["cmd+minus"],
        Window,
    ),
    action(
        "reset_font_size",
        "Reset font size",
        &["cmd+digit_0"],
        Window,
    ),
    action(
        "toggle_command_palette",
        "Command palette",
        &["cmd+shift+p"],
        Window,
    ),
    action("open_config", "Open settings", &["cmd+comma"], Window),
    action(
        "reload_config",
        "Reload settings file",
        &["cmd+shift+comma"],
        Window,
    ),
    action(
        "toggle_fullscreen",
        "Toggle full screen",
        &["cmd+enter", "cmd+ctrl+f"],
        Window,
    ),
    action(
        "restart_session",
        "Restart failed session",
        &["cmd+shift+r"],
        Window,
    ),
    action(
        "toggle_tab_sidebar",
        "Toggle tabs sidebar",
        &["cmd+b", "cmd+shift+b"],
        Window,
    ),
    action(
        "toggle_tools_panel",
        "Toggle tools panel",
        &["cmd+backslash"],
        Window,
    ),
    action(
        "show_project_explorer",
        "Project explorer",
        &["ctrl+digit_1"],
        Window,
    ),
    action(
        "show_global_search",
        "Global search",
        &["ctrl+digit_3"],
        Window,
    ),
    action("find_in_files", "Find in files", &["cmd+shift+f"], Window),
    action("group_tab", "Add tab to new group", &["cmd+g"], Window),
    action(
        "ungroup_tab",
        "Remove tab from group",
        &["cmd+shift+g"],
        Window,
    ),
];

pub fn find(name: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|action| action.name == name)
}

/// A key and its modifiers, compared regardless of how the line spelled them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Trigger {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// A Ghostty key name: `a`, `digit_1`, `bracket_left`, `arrow_up`, `enter`, …
    pub key: String,
}

/// Ghostty key names and GPUI's spelling of the same key.
const KEYS: &[(&str, &str)] = &[
    ("equal", "="),
    ("minus", "-"),
    ("plus", "+"),
    ("comma", ","),
    ("period", "."),
    ("slash", "/"),
    ("backslash", "\\"),
    ("semicolon", ";"),
    ("quote", "'"),
    ("backquote", "`"),
    ("bracket_left", "["),
    ("bracket_right", "]"),
    ("arrow_up", "up"),
    ("arrow_down", "down"),
    ("arrow_left", "left"),
    ("arrow_right", "right"),
    ("page_up", "pageup"),
    ("page_down", "pagedown"),
    ("home", "home"),
    ("end", "end"),
    ("enter", "enter"),
    ("escape", "escape"),
    ("tab", "tab"),
    ("space", "space"),
    ("backspace", "backspace"),
    ("delete", "delete"),
];

/// US-layout characters typed with Shift, which GPUI matches as the shifted character.
const SHIFTED: &[(&str, &str)] = &[
    ("[", "{"),
    ("]", "}"),
    (",", "<"),
    (".", ">"),
    ("/", "?"),
    (";", ":"),
    ("'", "\""),
    ("-", "_"),
    ("=", "+"),
    ("`", "~"),
    ("\\", "|"),
    ("1", "!"),
    ("2", "@"),
    ("3", "#"),
    ("4", "$"),
    ("5", "%"),
    ("6", "^"),
    ("7", "&"),
    ("8", "*"),
    ("9", "("),
    ("0", ")"),
];

impl Trigger {
    /// Parses Ghostty's `cmd+shift+d` form; GPUI spellings of keys (`=`, `up`) are accepted.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_lowercase();
        let (mods, key) = match text.strip_suffix("++") {
            Some(mods) => (mods, "+"),
            None => text.rsplit_once('+').unwrap_or(("", text.as_str())),
        };
        let mut trigger = Self {
            ctrl: false,
            alt: false,
            shift: false,
            cmd: false,
            key: String::new(),
        };
        for part in mods.split('+').filter(|part| !part.is_empty()) {
            match part {
                "ctrl" | "control" => trigger.ctrl = true,
                "alt" | "opt" | "option" => trigger.alt = true,
                "shift" => trigger.shift = true,
                "cmd" | "command" | "super" => trigger.cmd = true,
                _ => return None,
            }
        }
        trigger.key = key_name(key, &mut trigger.shift)?;
        Some(trigger)
    }

    /// Builds a trigger from what GPUI reports for a key press.
    pub fn from_keystroke(
        ctrl: bool,
        alt: bool,
        shift: bool,
        cmd: bool,
        key: &str,
    ) -> Option<Self> {
        let mut shift = shift;
        let key = key_name(&key.to_lowercase(), &mut shift)?;
        Some(Self {
            ctrl,
            alt,
            shift,
            cmd,
            key,
        })
    }

    /// GPUI's keystroke text, e.g. `cmd-shift-[` becomes `cmd-{`.
    pub fn to_gpui(&self) -> String {
        let mut key = KEYS.iter().find(|(name, _)| *name == self.key).map_or_else(
            || {
                self.key
                    .strip_prefix("digit_")
                    .unwrap_or(&self.key)
                    .to_owned()
            },
            |(_, gpui)| (*gpui).to_owned(),
        );
        let mut shift = self.shift;
        if shift && let Some((_, shifted)) = SHIFTED.iter().find(|(plain, _)| *plain == key) {
            key = (*shifted).to_owned();
            shift = false;
        }
        let mut parts = Vec::new();
        for (on, name) in [
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (shift, "shift"),
            (self.cmd, "cmd"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&key);
        parts.join("-")
    }

    /// The symbols shown on key caps, in macOS order.
    pub fn caps(&self) -> Vec<String> {
        let mut caps: Vec<String> = [
            (self.ctrl, "⌃"),
            (self.alt, "⌥"),
            (self.shift, "⇧"),
            (self.cmd, "⌘"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, cap)| cap.to_owned())
        .collect();
        let key = match self.key.as_str() {
            "arrow_up" => "↑".into(),
            "arrow_down" => "↓".into(),
            "arrow_left" => "←".into(),
            "arrow_right" => "→".into(),
            "enter" => "↩".into(),
            "escape" => "Esc".into(),
            "tab" => "Tab".into(),
            "space" => "Space".into(),
            "backspace" => "⌫".into(),
            "delete" => "⌦".into(),
            "page_up" => "PgUp".into(),
            "page_down" => "PgDn".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            key => match KEYS.iter().find(|(name, _)| *name == key) {
                Some((_, symbol)) => (*symbol).to_owned(),
                None => key.strip_prefix("digit_").unwrap_or(key).to_uppercase(),
            },
        };
        caps.push(key);
        caps
    }
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (on, name) in [
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.cmd, "cmd"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&self.key)
    }
}

/// A Ghostty key name for `key`, turning a shifted symbol back into Shift and its key.
fn key_name(key: &str, shift: &mut bool) -> Option<String> {
    if let Some((plain, _)) = SHIFTED.iter().find(|(_, shifted)| *shifted == key)
        && key != "+"
    {
        *shift = true;
        return key_name(plain, shift);
    }
    if let Some((name, _)) = KEYS
        .iter()
        .find(|(name, gpui)| *gpui == key || *name == key)
    {
        return Some((*name).to_owned());
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(digit @ '0'..='9'), None) => Some(format!("digit_{digit}")),
        (Some('a'..='z'), None) => Some(key.to_owned()),
        _ if key.starts_with("digit_")
            || (key.starts_with('f') && key[1..].parse::<u8>().is_ok()) =>
        {
            Some(key.to_owned())
        }
        _ => None,
    }
}

/// The bindings in effect: defaults, then each `keybind` line in order. A trigger runs one
/// action; `trigger=unbind` removes it. Returns the bindings and a warning per bad line.
pub fn resolve(lines: &[String]) -> (BTreeMap<Trigger, &'static str>, Vec<String>) {
    let mut bindings = BTreeMap::new();
    for action in ACTIONS {
        for default in action.defaults {
            if let Some(trigger) = Trigger::parse(default) {
                bindings.insert(trigger, action.name);
            }
        }
    }
    let mut warnings = Vec::new();
    for line in lines {
        let parsed = split_line(line).and_then(|(trigger, target)| {
            let trigger = Trigger::parse(trigger)?;
            match target {
                "unbind" => Some((trigger, None)),
                name => Some((trigger, Some(find(name)?.name))),
            }
        });
        match parsed {
            Some((trigger, Some(name))) => {
                bindings.insert(trigger, name);
            }
            Some((trigger, None)) => {
                bindings.remove(&trigger);
            }
            None => warnings.push(format!("keybind = {line}: unknown key or action")),
        }
    }
    (bindings, warnings)
}

/// `cmd+shift+d=new_split:down` → trigger and action. The trigger can end in `=` or `+`.
fn split_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let at = line
        .find("+=")
        .map(|at| at + 1)
        .or_else(|| line.find('='))?;
    let (trigger, target) = (line[..at].trim(), line[at + 1..].trim());
    (!trigger.is_empty() && !target.is_empty()).then_some((trigger, target))
}

/// The triggers bound to each action, for the Keyboard shortcuts list.
pub fn by_action(
    bindings: &BTreeMap<Trigger, &'static str>,
) -> BTreeMap<&'static str, Vec<Trigger>> {
    let mut out: BTreeMap<&'static str, Vec<Trigger>> = BTreeMap::new();
    for (trigger, name) in bindings {
        out.entry(name).or_default().push(trigger.clone());
    }
    out
}

/// Rewrites the settings file's keybind lines for `name`: drops lines that bound it and
/// lines that unbound its defaults, then, unless resetting, unbinds the defaults and binds
/// `trigger` (none clears the action). Other lines and comments are kept.
pub fn write_binding(text: &str, name: &str, change: Option<Option<&Trigger>>) -> String {
    let Some(action) = find(name) else {
        return text.to_owned();
    };
    let defaults: Vec<Trigger> = action
        .defaults
        .iter()
        .filter_map(|d| Trigger::parse(d))
        .collect();
    let mine = |line: &str| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        if key.trim() != "keybind" || line.trim_start().starts_with('#') {
            return false;
        }
        match split_line(value) {
            Some((_, target)) if target == name => true,
            Some((trigger, "unbind")) => {
                Trigger::parse(trigger).is_some_and(|trigger| defaults.contains(&trigger))
            }
            _ => false,
        }
    };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| !mine(line))
        .map(str::to_owned)
        .collect();
    if let Some(new) = change {
        for default in &defaults {
            if Some(default) != new {
                lines.push(format!("keybind = {default}=unbind"));
            }
        }
        if let Some(new) = new
            && !defaults.contains(new)
        {
            lines.push(format!("keybind = {new}={name}"));
        }
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Trigger {
        Trigger::parse(text).unwrap()
    }

    #[test]
    fn triggers_convert_between_ghostty_and_gpui() {
        assert_eq!(t("cmd+shift+d").to_gpui(), "shift-cmd-d");
        assert_eq!(t("cmd+shift+bracket_left").to_gpui(), "cmd-{");
        assert_eq!(t("cmd+digit_1").to_gpui(), "cmd-1");
        assert_eq!(t("cmd+alt+arrow_up").to_gpui(), "alt-cmd-up");
        assert_eq!(t("cmd+backslash").to_gpui(), "cmd-\\");
        assert_eq!(t("cmd+plus").to_gpui(), "cmd-+");
        assert_eq!(t("cmd++"), t("cmd+plus"));
        assert_eq!(t("Command+Shift+D"), t("shift+cmd+d"));
        assert_eq!(t("cmd+{"), t("cmd+shift+bracket_left"));
        assert_eq!(t("cmd+="), t("cmd+equal"));
        assert!(Trigger::parse("hyper+d").is_none());
        assert!(Trigger::parse("cmd+what").is_none());
        let typed = Trigger::from_keystroke(false, false, true, true, "{").unwrap();
        assert_eq!(typed, t("cmd+shift+bracket_left"));
        assert_eq!(typed.to_string(), "shift+cmd+bracket_left");
        assert_eq!(t("cmd+shift+enter").caps(), ["⇧", "⌘", "↩"]);
    }

    #[test]
    fn every_default_parses_and_is_unique() {
        let (bindings, warnings) = resolve(&[]);
        assert!(warnings.is_empty());
        let defaults: usize = ACTIONS.iter().map(|action| action.defaults.len()).sum();
        assert_eq!(bindings.len(), defaults, "no default trigger is used twice");
        assert_eq!(bindings[&t("cmd+t")], "new_tab");
    }

    #[test]
    fn keybind_lines_rebind_unbind_and_report_errors() {
        let lines = [
            "cmd+shift+t=new_tab".to_owned(),
            "cmd+t=unbind".to_owned(),
            "cmd+k=new_split:right".to_owned(),
            "cmd+nope=new_tab".to_owned(),
            "cmd+j=launch_rockets".to_owned(),
        ];
        let (bindings, warnings) = resolve(&lines);
        assert_eq!(bindings[&t("cmd+shift+t")], "new_tab");
        assert!(!bindings.contains_key(&t("cmd+t")));
        assert_eq!(bindings[&t("cmd+k")], "new_split:right", "later line wins");
        assert_eq!(warnings.len(), 2);
        let actions = by_action(&bindings);
        assert_eq!(actions["new_split:right"], [t("cmd+d"), t("cmd+k")]);
        assert!(!actions.contains_key("clear_screen"));
    }

    #[test]
    fn editing_a_binding_rewrites_only_its_lines() {
        let text = "# mine\ntheme = light\n";
        let set = write_binding(text, "new_tab", Some(Some(&t("cmd+shift+t"))));
        assert_eq!(
            set,
            "# mine\ntheme = light\nkeybind = cmd+t=unbind\nkeybind = shift+cmd+t=new_tab\n"
        );
        let (bindings, _) = resolve(&Settings::keybinds_of(&set));
        assert_eq!(bindings.get(&t("cmd+shift+t")), Some(&"new_tab"));
        assert!(!bindings.contains_key(&t("cmd+t")));
        // Saving again replaces rather than piles up lines.
        let again = write_binding(&set, "new_tab", Some(Some(&t("cmd+y"))));
        assert_eq!(again.matches("keybind").count(), 2);
        let cleared = write_binding(&again, "new_tab", Some(None));
        assert_eq!(cleared, "# mine\ntheme = light\nkeybind = cmd+t=unbind\n");
        assert_eq!(write_binding(&cleared, "new_tab", None), text);
        // Picking the default back writes nothing extra for it.
        let back = write_binding(text, "new_tab", Some(Some(&t("cmd+t"))));
        assert_eq!(back, text);
        let two = write_binding(text, "next_tab", Some(Some(&t("cmd+l"))));
        assert_eq!(two.matches("=unbind").count(), 2);
    }

    struct Settings;
    impl Settings {
        fn keybinds_of(text: &str) -> Vec<String> {
            crate::settings::Settings::parse(text).0.keybinds
        }
    }
}
