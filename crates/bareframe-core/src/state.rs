//! The saved window layout: tabs, groups, splits and what each pane showed.
//!
//! One record per line, space-separated, with text fields percent-encoded so names and
//! paths may hold any byte. An unreadable file is reported and ignored by the caller.
use crate::{
    layout::{Axis, Layout, MIN_RATIO, PaneId},
    workspace::{Group, GroupId, Tab, TabId, Workspace},
};
use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
};

const HEADER: &str = "bareframe-state 1";

#[derive(Clone, Debug, PartialEq)]
pub enum SavedPane {
    /// A terminal reopens as a fresh shell in its last directory.
    Terminal {
        cwd: PathBuf,
    },
    Settings,
    /// A Markdown file in the viewer.
    Markdown {
        path: PathBuf,
    },
    /// Files in the code editor, as tabs; `active` is the one shown. Unsaved changes
    /// are not kept.
    Editor {
        paths: Vec<PathBuf>,
        active: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedState {
    pub workspace: Workspace,
    pub panes: Vec<(PaneId, SavedPane)>,
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    pub tools_open: bool,
    pub tools_width: f32,
    /// The Code review panel on the window's right.
    pub review_open: bool,
    pub review_width: f32,
    /// Origin and size in points.
    pub window: Option<[f32; 4]>,
}

pub fn encode(state: &SavedState) -> String {
    let mut out = vec![HEADER.to_owned()];
    if let Some([x, y, width, height]) = state.window {
        out.push(format!("window {x} {y} {width} {height}"));
    }
    out.push(format!(
        "sidebar {} {}",
        flag(state.sidebar_open),
        state.sidebar_width
    ));
    out.push(format!(
        "tools {} {}",
        flag(state.tools_open),
        state.tools_width
    ));
    out.push(format!(
        "review {} {}",
        flag(state.review_open),
        state.review_width
    ));
    out.push(format!("active {}", state.workspace.active_tab));
    for group in &state.workspace.groups {
        out.push(format!(
            "group {} {} {} {} {}",
            group.id.0,
            group.color,
            flag(group.collapsed),
            flag(group.pinned),
            text(group.name.as_deref()),
        ));
    }
    for tab in &state.workspace.tabs {
        let mut layout = String::new();
        write_layout(&tab.layout, &mut layout);
        out.push(format!(
            "tab {} {} {} {} {} {} {}{}",
            tab.id.0,
            tab.group.map_or("-".into(), |group| group.0.to_string()),
            flag(tab.pinned),
            tab.color.map_or("-".into(), |color| color.to_string()),
            flag(tab.maximized),
            tab.active_pane.0,
            text(tab.title.as_deref()),
            layout,
        ));
    }
    for (id, pane) in &state.panes {
        out.push(match pane {
            SavedPane::Terminal { cwd } => {
                format!(
                    "pane {} terminal {}",
                    id.0,
                    escape(cwd.as_os_str().as_bytes())
                )
            }
            SavedPane::Settings => format!("pane {} settings", id.0),
            SavedPane::Markdown { path } => {
                format!(
                    "pane {} markdown {}",
                    id.0,
                    escape(path.as_os_str().as_bytes())
                )
            }
            SavedPane::Editor { paths, active } => {
                let paths: Vec<String> = paths
                    .iter()
                    .map(|path| escape(path.as_os_str().as_bytes()))
                    .collect();
                format!("pane {} editor {active} {}", id.0, paths.join(" "))
            }
        });
    }
    out.push(String::new());
    out.join("\n")
}

pub fn decode(input: &str) -> Result<SavedState, String> {
    let mut lines = input.lines().enumerate();
    if lines.next().map(|(_, line)| line) != Some(HEADER) {
        return Err("not a bareframe state file".into());
    }
    let mut window = None;
    let (mut sidebar_open, mut sidebar_width) = (true, 248.0);
    let (mut tools_open, mut tools_width) = (false, 260.0);
    let (mut review_open, mut review_width) = (false, 520.0);
    let mut active = 0;
    let (mut groups, mut tabs, mut panes) = (Vec::new(), Vec::new(), Vec::new());
    for (number, line) in lines {
        let fail = |what: &str| format!("line {}: {what}", number + 1);
        let mut words = line.split(' ');
        let mut next = || words.next().ok_or_else(|| fail("missing field"));
        match next()? {
            "" => {}
            "window" => {
                let mut values = [0.0; 4];
                for value in &mut values {
                    *value = size_of(next()?).ok_or_else(|| fail("bad window"))?;
                }
                window = Some(values);
            }
            "sidebar" => {
                sidebar_open = flag_of(next()?).ok_or_else(|| fail("bad sidebar"))?;
                sidebar_width = size_of(next()?).ok_or_else(|| fail("bad width"))?;
            }
            "tools" => {
                tools_open = flag_of(next()?).ok_or_else(|| fail("bad tools"))?;
                tools_width = size_of(next()?).ok_or_else(|| fail("bad width"))?;
            }
            "review" => {
                review_open = flag_of(next()?).ok_or_else(|| fail("bad review"))?;
                review_width = size_of(next()?).ok_or_else(|| fail("bad width"))?;
            }
            "active" => active = number_of(next()?).ok_or_else(|| fail("bad active tab"))?,
            "group" => groups.push(Group {
                id: GroupId(number_of(next()?).ok_or_else(|| fail("bad group id"))?),
                color: number_of(next()?).ok_or_else(|| fail("bad color"))?,
                collapsed: flag_of(next()?).ok_or_else(|| fail("bad flag"))?,
                pinned: flag_of(next()?).ok_or_else(|| fail("bad flag"))?,
                name: text_of(next()?).ok_or_else(|| fail("bad name"))?,
            }),
            "tab" => {
                let id = TabId(number_of(next()?).ok_or_else(|| fail("bad tab id"))?);
                let group = optional(next()?).ok_or_else(|| fail("bad group"))?;
                let pinned = flag_of(next()?).ok_or_else(|| fail("bad flag"))?;
                let color = optional(next()?).ok_or_else(|| fail("bad color"))?;
                let maximized = flag_of(next()?).ok_or_else(|| fail("bad flag"))?;
                let active_pane = PaneId(number_of(next()?).ok_or_else(|| fail("bad pane"))?);
                let title = text_of(next()?).ok_or_else(|| fail("bad title"))?;
                let mut tokens = std::iter::from_fn(|| words.next());
                let layout = read_layout(&mut tokens).ok_or_else(|| fail("bad layout"))?;
                if tokens.next().is_some() {
                    return Err(fail("extra layout"));
                }
                let mut tab = Tab::restored(id, layout, active_pane);
                tab.group = group.map(GroupId);
                tab.pinned = pinned;
                tab.color = color;
                tab.maximized = maximized;
                tab.title = title;
                tabs.push(tab);
            }
            "pane" => {
                let id = PaneId(number_of(next()?).ok_or_else(|| fail("bad pane id"))?);
                let pane = match next()? {
                    "terminal" => SavedPane::Terminal {
                        cwd: PathBuf::from(OsString::from_vec(
                            unescape(next()?).ok_or_else(|| fail("bad path"))?,
                        )),
                    },
                    "settings" => SavedPane::Settings,
                    "markdown" => SavedPane::Markdown {
                        path: PathBuf::from(OsString::from_vec(
                            unescape(next()?).ok_or_else(|| fail("bad path"))?,
                        )),
                    },
                    "editor" => {
                        let active = number_of(next()?).ok_or_else(|| fail("bad tab"))?;
                        let mut paths = Vec::new();
                        while let Ok(word) = next() {
                            let path = unescape(word).ok_or_else(|| fail("bad path"))?;
                            paths.push(PathBuf::from(OsString::from_vec(path)));
                        }
                        if paths.is_empty() {
                            return Err(fail("no files"));
                        }
                        SavedPane::Editor { paths, active }
                    }
                    other => return Err(fail(&format!("unknown pane kind `{other}`"))),
                };
                panes.push((id, pane));
            }
            other => return Err(fail(&format!("unknown record `{other}`"))),
        }
    }
    let workspace = Workspace::from_parts(tabs, groups, active)
        .ok_or_else(|| "inconsistent tabs or groups".to_owned())?;
    Ok(SavedState {
        workspace,
        panes,
        sidebar_open,
        sidebar_width,
        tools_open,
        tools_width,
        review_open,
        review_width,
        window,
    })
}

fn write_layout(layout: &Layout, out: &mut String) {
    match layout {
        Layout::Pane(id) => out.push_str(&format!(" p{}", id.0)),
        Layout::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let axis = match axis {
                Axis::Horizontal => 'h',
                Axis::Vertical => 'v',
            };
            out.push_str(&format!(" {axis}{ratio}"));
            write_layout(first, out);
            write_layout(second, out);
        }
    }
}

/// Prefix order: a split token is followed by its two children.
// ponytail: recursion depth equals split depth; a hand-made file nesting thousands of
// splits could overflow the stack. Cap the depth if state files ever come from elsewhere.
fn read_layout<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<Layout> {
    let token = tokens.next()?;
    let (kind, value) = token.split_at_checked(1)?;
    match kind {
        "p" => Some(Layout::Pane(PaneId(value.parse().ok()?))),
        "h" | "v" => {
            let ratio: f32 = value.parse().ok()?;
            if !ratio.is_finite() {
                return None;
            }
            Some(Layout::Split {
                axis: if kind == "h" {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                },
                ratio: ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO),
                first: Box::new(read_layout(tokens)?),
                second: Box::new(read_layout(tokens)?),
            })
        }
        _ => None,
    }
}

fn flag(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn flag_of(word: &str) -> Option<bool> {
    match word {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn number_of<T: std::str::FromStr>(word: &str) -> Option<T> {
    word.parse().ok()
}

/// A position or size: `NaN` and infinities parse as `f32` but can't place a window.
fn size_of(word: &str) -> Option<f32> {
    number_of(word).filter(|value: &f32| value.is_finite())
}

fn optional<T: std::str::FromStr>(word: &str) -> Option<Option<T>> {
    if word == "-" {
        Some(None)
    } else {
        word.parse().ok().map(Some)
    }
}

/// `-` for none, `=` and the escaped text otherwise, so an empty or `-` name survives.
fn text(value: Option<&str>) -> String {
    value.map_or("-".into(), |value| format!("={}", escape(value.as_bytes())))
}

fn text_of(word: &str) -> Option<Option<String>> {
    if word == "-" {
        return Some(None);
    }
    String::from_utf8(unescape(word.strip_prefix('=')?)?)
        .ok()
        .map(Some)
}

/// Percent-encodes everything but unreserved characters and `/`; also used for `file://` URIs.
pub(crate) fn escape(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte.is_ascii_alphanumeric() || b"/._-~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(crate) fn unescape(word: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(word.len());
    let mut input = word.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hex = [input.next()?, input.next()?];
            bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SavedState {
        let (mut ws, first) = Workspace::new();
        let right = ws.split(Axis::Horizontal);
        ws.split(Axis::Vertical);
        ws.focus(right);
        ws.toggle_maximized();
        let second = ws.new_tab();
        ws.new_group(&[1]);
        let group = ws.tabs[1].group.unwrap();
        ws.rename_group(group, "작업 공간");
        ws.rename_tab(ws.tabs[0].id, "a b%c");
        ws.set_tab_color(0, Some(3));
        ws.set_tab_pinned(0, true);
        let mut panes: Vec<(PaneId, SavedPane)> = ws
            .panes()
            .map(|id| {
                (
                    id,
                    SavedPane::Terminal {
                        cwd: PathBuf::from("/tmp/with space/한글"),
                    },
                )
            })
            .collect();
        panes.retain(|(id, _)| *id != second);
        panes.push((second, SavedPane::Settings));
        // A third tab showing a Markdown file.
        let third = ws.new_tab();
        panes.push((
            third,
            SavedPane::Markdown {
                path: PathBuf::from("/tmp/문서 a.md"),
            },
        ));
        let fourth = ws.new_tab();
        panes.push((
            fourth,
            SavedPane::Editor {
                paths: vec![
                    PathBuf::from("/tmp/src/main.rs"),
                    PathBuf::from("/tmp/a b.rs"),
                ],
                active: 1,
            },
        ));
        assert!(ws.tabs.iter().any(|tab| tab.layout.contains(first)));
        SavedState {
            workspace: ws,
            panes,
            sidebar_open: false,
            sidebar_width: 300.5,
            tools_open: true,
            tools_width: 280.0,
            review_open: true,
            review_width: 600.0,
            window: Some([10.0, 20.0, 1040.0, 720.0]),
        }
    }

    #[test]
    fn a_saved_layout_reads_back_unchanged() {
        let state = sample();
        let text = encode(&state);
        assert_eq!(decode(&text), Ok(state));
    }

    #[test]
    fn restored_ids_do_not_collide_with_new_ones() {
        let mut state = decode(&encode(&sample())).unwrap();
        let used: Vec<PaneId> = state.workspace.panes().collect();
        let fresh = state.workspace.new_tab();
        assert!(!used.contains(&fresh));
    }

    #[test]
    fn broken_files_are_rejected_not_half_loaded() {
        assert!(decode("").is_err());
        assert!(decode("something else").is_err());
        assert!(decode(&format!("{HEADER}\nactive 0\n")).is_err(), "no tabs");
        assert!(decode(&format!("{HEADER}\ntab 1 - 0 - 0 1 - h0.5 p1\n")).is_err());
        // The same pane in two tabs.
        let twice = format!("{HEADER}\ntab 1 - 0 - 0 1 - p1\ntab 2 - 0 - 0 1 - p1\n");
        assert!(decode(&twice).is_err());
        // A tab in a group that does not exist.
        assert!(decode(&format!("{HEADER}\ntab 1 7 0 - 0 1 - p1\n")).is_err());
        assert!(decode(&format!("{HEADER}\nbogus\n")).is_err());
        // Colors index the theme's palette, so one past the end would crash the first render.
        assert!(decode(&format!("{HEADER}\ntab 1 - 0 6 0 1 - p1\n")).is_err());
        let group = format!("{HEADER}\ngroup 1 9 0 0 -\ntab 1 1 0 - 0 1 - p1\n");
        assert!(decode(&group).is_err());
        assert!(decode(&format!("{HEADER}\nsidebar 1 NaN\ntab 1 - 0 - 0 1 - p1\n")).is_err());
        assert!(
            decode(&format!(
                "{HEADER}\nwindow 0 0 inf 720\ntab 1 - 0 - 0 1 - p1\n"
            ))
            .is_err()
        );
        // The same lines with good values load.
        assert!(
            decode(&format!(
                "{HEADER}\ngroup 1 5 0 0 -\ntab 1 1 0 5 0 1 - p1\n"
            ))
            .is_ok()
        );
    }
}
