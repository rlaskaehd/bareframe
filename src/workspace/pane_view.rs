//! What each pane shows, and what the sidebar, the pane headers and the window title say
//! about it.
use crate::{
    editor_tabs::EditorTabs,
    file_icons::{self, FileIcon},
    files,
    markdown::MarkdownPane,
    pane::TerminalPane,
    settings_pane::SettingsPane,
};
use gpui::{AnyView, App, Entity, Window};

/// What a pane shows. The layout model only knows `PaneId`s; native surfaces exist only
/// for terminals, so hiding and freezing apply to them alone.
#[derive(Clone)]
pub(super) enum PaneView {
    Terminal(Entity<TerminalPane>),
    Settings(Entity<SettingsPane>),
    Markdown(Entity<MarkdownPane>),
    Editor(Entity<EditorTabs>),
}

impl PaneView {
    pub(super) fn terminal(&self) -> Option<&Entity<TerminalPane>> {
        match self {
            Self::Terminal(pane) => Some(pane),
            Self::Settings(_) | Self::Markdown(_) | Self::Editor(_) => None,
        }
    }

    /// The pane header and window title.
    pub(super) fn title(&self, cx: &App) -> String {
        match self {
            Self::Terminal(pane) => pane.read(cx).title(),
            Self::Settings(_) => "Settings".into(),
            Self::Markdown(pane) => pane.read(cx).file_name(),
            Self::Editor(tabs) => editor_name(tabs, cx),
        }
    }

    /// The sidebar card's first line.
    pub(super) fn name(&self, cx: &App) -> String {
        match self {
            Self::Terminal(pane) => pane.read(cx).name(),
            Self::Settings(_) => "Settings".into(),
            Self::Markdown(pane) => pane.read(cx).file_name(),
            Self::Editor(tabs) => editor_name(tabs, cx),
        }
    }

    /// The file a file pane shows: the viewer's, or the editor's shown tab.
    pub(super) fn file(&self, cx: &App) -> Option<std::path::PathBuf> {
        match self {
            Self::Markdown(pane) => Some(pane.read(cx).path().to_path_buf()),
            Self::Editor(tabs) => {
                let editor = tabs.read(cx).active()?;
                Some(editor.read(cx).path().to_path_buf())
            }
            _ => None,
        }
    }

    /// The sidebar card's second line.
    pub(super) fn location(&self, cx: &App) -> Option<String> {
        match self {
            Self::Terminal(pane) => Some(pane.read(cx).display_cwd()),
            Self::Settings(_) => None,
            Self::Markdown(_) | Self::Editor(_) => {
                let path = self.file(cx)?;
                let dir = path.parent()?;
                Some(files::home_dir().map_or_else(
                    || dir.display().to_string(),
                    |home| bareframe_core::config::display_path(dir, &home),
                ))
            }
        }
    }

    /// The sidebar card's icon in place of the prompt glyph: a file shows its type.
    pub(super) fn icon(&self, cx: &App) -> Option<FileIcon> {
        match self {
            Self::Terminal(_) => None,
            Self::Settings(_) => Some(FileIcon::SETTINGS),
            Self::Markdown(_) | Self::Editor(_) => Some(
                self.file(cx)
                    .map_or(FileIcon::FILE, |file| file_icons::for_path(&file)),
            ),
        }
    }

    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        match self {
            Self::Terminal(pane) => pane.update(cx, |pane, cx| pane.focus(window, cx)),
            Self::Settings(pane) => pane.update(cx, |pane, cx| pane.focus(window, cx)),
            Self::Markdown(pane) => pane.update(cx, |pane, cx| pane.focus(window, cx)),
            Self::Editor(tabs) => tabs.update(cx, |tabs, cx| tabs.focus(window, cx)),
        }
    }

    pub(super) fn view(&self) -> AnyView {
        match self {
            Self::Terminal(pane) => pane.clone().into(),
            Self::Settings(pane) => pane.clone().into(),
            Self::Markdown(pane) => pane.clone().into(),
            Self::Editor(pane) => pane.clone().into(),
        }
    }
}

/// The editor's shown file, with a dot if it has unsaved changes.
fn editor_name(tabs: &Entity<EditorTabs>, cx: &App) -> String {
    let tabs = tabs.read(cx);
    tabs.active()
        .map_or_else(String::new, |editor| editor.read(cx).file_name())
}
