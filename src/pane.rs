use crate::{terminal::adapter::GhosttyAdapter, theme};
use bareframe_core::{
    config::{LaunchConfig, display_path},
    session::{
        ClipboardOperation, SessionId, SessionState, SessionStatus, TerminalEvent,
        TerminalEventKind,
    },
    workspace::PaneId,
};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, RenderImage, Subscription, Window, div, img,
    prelude::*,
};
use gpui_libghostty::TerminalTheme;
use std::sync::Arc;

pub enum PaneEvent {
    /// The shell ended; the pane should close.
    Exited,
    TitleChanged,
    /// The terminal waits for the user to allow or refuse a clipboard operation.
    Clipboard(ClipboardOperation, String),
}

/// One terminal session and the native surface showing it.
pub struct TerminalPane {
    config: LaunchConfig,
    session: SessionState,
    terminal: Option<Entity<GhosttyAdapter>>,
    backend: Option<Subscription>,
    focus: FocusHandle,
    /// Shown in place of the hidden surface while a popup overlaps it.
    frozen: Option<Arc<RenderImage>>,
    /// Snapshots no longer shown; GPUI keeps their textures until the window drops them.
    stale: Vec<Arc<RenderImage>>,
    /// The running or last finished command, from the shell integration's titles.
    last_command: Option<String>,
}

impl EventEmitter<PaneEvent> for TerminalPane {}

impl TerminalPane {
    pub fn new(
        id: PaneId,
        config: LaunchConfig,
        theme: &TerminalTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut pane = Self {
            session: SessionState::new(SessionId(id.0), config.cwd.clone()),
            config,
            terminal: None,
            backend: None,
            focus: cx.focus_handle(),
            frozen: None,
            stale: Vec::new(),
            last_command: None,
        };
        pane.start(theme, window, cx);
        pane
    }

    fn start(&mut self, theme: &TerminalTheme, window: &mut Window, cx: &mut Context<Self>) {
        match GhosttyAdapter::spawn(&self.config, self.session.key, theme, window, cx) {
            Ok(terminal) => {
                self.backend = Some(cx.subscribe(
                    &terminal,
                    |pane, _, event: &TerminalEvent, cx| {
                        if !pane.session.apply(event) {
                            return;
                        }
                        if let TerminalEventKind::TitleChanged(title) = &event.kind
                            && title != "Terminal"
                            && !is_location(title)
                        {
                            pane.last_command = Some(title.clone());
                        }
                        match &event.kind {
                            TerminalEventKind::ClipboardRequest { operation, text } => {
                                cx.emit(PaneEvent::Clipboard(*operation, text.clone()))
                            }
                            TerminalEventKind::Exited { .. } => cx.emit(PaneEvent::Exited),
                            TerminalEventKind::TitleChanged(_)
                            | TerminalEventKind::WorkingDirectoryChanged(_) => {
                                cx.emit(PaneEvent::TitleChanged)
                            }
                            _ => {}
                        }
                        cx.notify();
                    },
                ));
                self.terminal = Some(terminal);
            }
            Err(error) => {
                self.focus.focus(window, cx);
                self.session.apply(&TerminalEvent {
                    key: self.session.key,
                    kind: TerminalEventKind::Failed(error),
                });
            }
        }
    }

    /// Only a pane whose shell could not start is restarted; an exited pane closes.
    pub fn retry(&mut self, theme: &TerminalTheme, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.session.status, SessionStatus::Failed(_))
            || self.session.restart(self.config.cwd.clone()).is_err()
        {
            return;
        }
        self.release(cx);
        self.start(theme, window, cx);
        cx.notify();
    }

    /// The shell integration's title: the `~` path at a prompt, the command while one runs.
    pub fn title(&self) -> String {
        match &self.session.status {
            SessionStatus::Failed(_) => "Could not start".into(),
            _ if self.session.title != "Terminal" => self.session.title.clone(),
            _ => self.display_cwd(),
        }
    }

    /// Whether the shell is waiting at its prompt: the shell integration titles the prompt
    /// with its location. Without the integration this is never known, so it is false.
    pub fn at_prompt(&self) -> bool {
        matches!(self.session.status, SessionStatus::Running) && is_location(&self.session.title)
    }

    /// The sidebar name, as Warp picks it: the running or last command, else "New session".
    pub fn name(&self) -> String {
        match &self.session.status {
            SessionStatus::Failed(_) => "Could not start".into(),
            _ => self
                .last_command
                .clone()
                .unwrap_or_else(|| "New session".into()),
        }
    }

    pub fn display_cwd(&self) -> String {
        match crate::app::home_dir() {
            Some(home) => display_path(&self.session.cwd, &home),
            None => self.session.cwd.display().to_string(),
        }
    }

    /// Where a tab opened from this pane starts. Known only if the shell reports it.
    pub fn cwd(&self) -> &std::path::Path {
        &self.session.cwd
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.terminal {
            Some(terminal) => terminal.update(cx, |terminal, cx| terminal.focus(window, cx)),
            None => self.focus.focus(window, cx),
        }
    }

    pub fn resolve_clipboard(&self, approved: bool, cx: &mut Context<Self>) {
        if let Some(terminal) = &self.terminal {
            terminal.update(cx, |terminal, _| terminal.resolve_clipboard(approved));
        }
    }

    pub fn paste(&self, cx: &mut Context<Self>) {
        if let Some(terminal) = &self.terminal {
            terminal.update(cx, |terminal, _| terminal.paste());
        }
    }

    pub fn type_text(&self, text: &str, cx: &mut Context<Self>) {
        if let Some(terminal) = &self.terminal {
            terminal.update(cx, |terminal, _| terminal.type_text(text));
        }
    }

    /// Native surfaces sit above GPUI and stay on screen unless hidden explicitly.
    pub fn set_visible(&self, visible: bool, cx: &mut Context<Self>) {
        if let Some(terminal) = &self.terminal {
            terminal.update(cx, |terminal, _| terminal.set_visible(visible));
        }
    }

    /// Swaps the native surface for a still image so GPUI can draw over it, or back.
    pub fn set_frozen(&mut self, frozen: bool, cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        self.stale.extend(self.frozen.take());
        if frozen {
            match terminal.update(cx, |terminal, _| terminal.snapshot()) {
                Ok(image) => self.frozen = Some(image),
                Err(error) => eprintln!("bareframe: {error}"),
            }
        }
        terminal.update(cx, |terminal, _| terminal.set_visible(!frozen));
        cx.notify();
    }

    pub fn apply_theme(&self, theme: &TerminalTheme, cx: &mut Context<Self>) {
        if let Some(terminal) = &self.terminal
            && let Err(error) = terminal.update(cx, |terminal, _| terminal.apply_theme(theme))
        {
            eprintln!("bareframe: {error}");
        }
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        self.backend.take();
        if let Some(terminal) = self.terminal.take() {
            terminal.update(cx, |terminal, _| terminal.shutdown());
        }
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        self.release(cx);
        self.session.close();
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        for image in self.stale.drain(..) {
            if let Err(error) = window.drop_image(image) {
                eprintln!("bareframe: releasing a terminal snapshot: {error}");
            }
        }
        let error = match &self.session.status {
            SessionStatus::Failed(error) => Some(format!(
                "Could not start terminal: {error}\nCmd+Shift+R to retry"
            )),
            _ => None,
        };
        div()
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .children(self.terminal.clone())
            .when_some(self.frozen.clone(), |this, image| {
                this.child(img(image).absolute().inset_0().size_full())
            })
            .when_some(error, |this, error| {
                this.p_4()
                    .font_family(theme::UI_FONT)
                    .text_sm()
                    .child(error)
            })
    }
}

/// The shell integration titles the prompt with its location (`~/a`, `/tmp`, `…/b/c/d`) and
/// a running command with the command line.
// ponytail: a command typed as a path (`/bin/ls`, `~/run.sh`) reads as a location; parse
// the integration's preexec hook directly if that matters.
fn is_location(title: &str) -> bool {
    title.starts_with('~') || title.starts_with('/') || title.starts_with("…/")
}

#[cfg(test)]
mod tests {
    use super::is_location;

    #[test]
    fn prompt_titles_are_locations_and_commands_are_not() {
        for title in ["~", "~/Sandbox/bareframe", "/tmp", "…/a/b/c"] {
            assert!(is_location(title), "{title}");
        }
        for title in ["vim notes.md", "cargo test", "htop"] {
            assert!(!is_location(title), "{title}");
        }
    }
}
