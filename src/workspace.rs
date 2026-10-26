use crate::{
    app::{CloseWindow, RestartSession},
    terminal::adapter::GhosttyAdapter,
};
use bareframe_core::{
    config::LaunchConfig,
    session::{SessionId, SessionState, SessionStatus, TerminalEvent, TerminalEventKind},
    workspace::WorkspaceState,
};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Subscription, Window, div, prelude::*, rgb,
};

/// Owns layout and session metadata, but no PTY, VT grid or native surface.
pub struct WorkspaceView {
    layout: WorkspaceState,
    config: LaunchConfig,
    session: SessionState,
    terminal: Option<Entity<GhosttyAdapter>>,
    backend_subscription: Option<Subscription>,
    _quit: Subscription,
    focus: FocusHandle,
}

impl EventEmitter<TerminalEvent> for WorkspaceView {}

impl WorkspaceView {
    pub fn new(config: LaunchConfig, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session = SessionState::new(SessionId(1), config.cwd.clone());
        let layout = WorkspaceState::single_terminal(&session);
        let mut view = Self {
            layout,
            config,
            session,
            terminal: None,
            backend_subscription: None,
            _quit: cx.on_app_quit(|view, cx| {
                view.shutdown(cx);
                async {}
            }),
            focus: cx.focus_handle(),
        };
        view.start(window, cx);
        view
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match GhosttyAdapter::spawn(&self.config, self.session.key, window, cx) {
            Ok(terminal) => {
                self.backend_subscription = Some(cx.subscribe(
                    &terminal,
                    |view, _, event: &TerminalEvent, cx| {
                        if view.session.apply(event) {
                            cx.emit(event.clone());
                            cx.notify();
                        }
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

    fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.restart(self.config.cwd.clone()).is_err() {
            return;
        }
        self.release_terminal(cx);
        self.start(window, cx);
        cx.notify();
    }

    fn release_terminal(&mut self, cx: &mut Context<Self>) {
        self.backend_subscription.take();
        if let Some(terminal) = self.terminal.take() {
            terminal.update(cx, |terminal, _| terminal.shutdown());
        }
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        self.release_terminal(cx);
        self.session.close();
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_title = match &self.session.status {
            SessionStatus::Running => format!("{} — {}", self.session.title, crate::app::name()),
            SessionStatus::Starting => format!("Starting — {}", crate::app::name()),
            SessionStatus::Exited { .. } => format!("Session ended — {}", crate::app::name()),
            SessionStatus::Failed(_) => format!("Could not start — {}", crate::app::name()),
            SessionStatus::Closed => format!("Closed — {}", crate::app::name()),
        };
        window.set_window_title(&window_title);
        let status = match &self.session.status {
            SessionStatus::Starting => "Starting zsh…".to_owned(),
            SessionStatus::Running => self.session.cwd.display().to_string(),
            SessionStatus::Exited { code } => code.map_or_else(
                || "Session ended · Cmd+Shift+R to restart".to_owned(),
                |code| format!("Session ended ({code}) · Cmd+Shift+R to restart"),
            ),
            SessionStatus::Failed(error) => {
                format!("Could not start terminal: {error} · Cmd+Shift+R to retry")
            }
            SessionStatus::Closed => "Session closed".to_owned(),
        };
        let active = self.layout.active_pane.0;
        let terminal = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .children(self.terminal.clone());
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x14161b))
            .font_family("Menlo")
            .text_color(rgb(0xd8dee9))
            .on_action(cx.listener(|view, _: &CloseWindow, window, cx| {
                view.shutdown(cx);
                window.remove_window();
            }))
            .on_action(cx.listener(|view, _: &RestartSession, window, cx| view.restart(window, cx)))
            .child(terminal)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h_8()
                    .flex_shrink_0()
                    .px_3()
                    .font_family("Menlo")
                    .text_sm()
                    .bg(rgb(0x1d2129))
                    .text_color(rgb(0xd8dee9))
                    .child(format!("{} · pane {active} · {status}", self.session.title)),
            )
    }
}
