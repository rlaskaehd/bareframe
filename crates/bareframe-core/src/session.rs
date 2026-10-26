use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// A restarted shell is a new run. Late callbacks must retain their original key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionKey {
    pub id: SessionId,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed(String),
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEventKind {
    Started,
    Exited {
        code: Option<i32>,
    },
    Failed(String),
    TitleChanged(String),
    WorkingDirectoryChanged(PathBuf),
    /// Advisory shell metadata, never the lifetime of the terminal session itself.
    CommandFinished {
        exit_code: Option<i32>,
        duration_millis: u64,
    },
    ObservationGap {
        dropped: u64,
    },
    /// The terminal asks the user before a clipboard operation goes ahead.
    ClipboardRequest {
        operation: ClipboardOperation,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardOperation {
    /// A paste Ghostty considers unsafe (a newline outside bracketed paste, or text
    /// that would end bracketed paste early).
    Paste,
    /// A program reading the clipboard through OSC 52.
    Read,
    /// A program writing the clipboard through OSC 52.
    Write,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalEvent {
    pub key: SessionKey,
    pub kind: TerminalEventKind,
}

#[derive(Clone, Debug)]
pub struct SessionState {
    pub key: SessionKey,
    pub status: SessionStatus,
    pub title: String,
    pub cwd: PathBuf,
}

impl SessionState {
    pub fn new(id: SessionId, cwd: PathBuf) -> Self {
        Self {
            key: SessionKey { id, generation: 1 },
            status: SessionStatus::Starting,
            title: "Terminal".into(),
            cwd,
        }
    }
    /// Returns false for stale events and invalid transitions, without mutating state.
    pub fn apply(&mut self, event: &TerminalEvent) -> bool {
        if event.key != self.key {
            return false;
        }
        match (&self.status, &event.kind) {
            (SessionStatus::Starting, TerminalEventKind::Started) => {
                self.status = SessionStatus::Running
            }
            (
                SessionStatus::Starting | SessionStatus::Running,
                TerminalEventKind::Exited { code },
            ) => self.status = SessionStatus::Exited { code: *code },
            (
                SessionStatus::Starting | SessionStatus::Running,
                TerminalEventKind::Failed(error),
            ) => self.status = SessionStatus::Failed(error.clone()),
            (SessionStatus::Running, TerminalEventKind::TitleChanged(title)) => {
                self.title = title.clone()
            }
            (SessionStatus::Running, TerminalEventKind::WorkingDirectoryChanged(cwd)) => {
                self.cwd = cwd.clone()
            }
            (SessionStatus::Running, TerminalEventKind::CommandFinished { .. }) => {}
            (SessionStatus::Running, TerminalEventKind::ObservationGap { .. }) => {}
            (SessionStatus::Running, TerminalEventKind::ClipboardRequest { .. }) => {}
            _ => return false,
        }
        true
    }
    pub fn restart(&mut self, initial_cwd: PathBuf) -> Result<SessionKey, &'static str> {
        if !matches!(
            self.status,
            SessionStatus::Exited { .. } | SessionStatus::Failed(_)
        ) {
            return Err("only an exited or failed session can restart");
        }
        self.key.generation = self
            .key
            .generation
            .checked_add(1)
            .ok_or("session generation exhausted")?;
        self.status = SessionStatus::Starting;
        self.cwd = initial_cwd;
        self.title = "Terminal".into();
        Ok(self.key)
    }
    pub fn close(&mut self) {
        self.status = SessionStatus::Closed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(state: &SessionState, kind: TerminalEventKind) -> TerminalEvent {
        TerminalEvent {
            key: state.key,
            kind,
        }
    }
    #[test]
    fn late_exit_from_previous_run_cannot_kill_restarted_shell() {
        let mut state = SessionState::new(SessionId(1), "/tmp".into());
        let exit = event(&state, TerminalEventKind::Exited { code: None });
        assert!(state.apply(&exit));
        let original_id = state.key.id;
        state.restart("/tmp".into()).unwrap();
        assert_eq!(state.key.id, original_id);
        assert!(!state.apply(&exit));
        assert_eq!(state.status, SessionStatus::Starting);
        let started = event(&state, TerminalEventKind::Started);
        assert!(state.apply(&started));
        assert_eq!(state.status, SessionStatus::Running);
    }
    #[test]
    fn command_failure_does_not_end_shell_and_unknown_exit_is_not_zero() {
        let mut state = SessionState::new(SessionId(1), "/tmp".into());
        assert!(state.apply(&event(&state, TerminalEventKind::Started)));
        assert!(state.apply(&event(
            &state,
            TerminalEventKind::CommandFinished {
                exit_code: Some(1),
                duration_millis: 10
            }
        )));
        assert_eq!(state.status, SessionStatus::Running);
        assert!(state.apply(&event(&state, TerminalEventKind::Exited { code: None })));
        assert_eq!(state.status, SessionStatus::Exited { code: None });
    }
    #[test]
    fn closed_session_cannot_be_resurrected_by_callbacks() {
        let mut state = SessionState::new(SessionId(2), "/tmp".into());
        state.close();
        assert!(!state.apply(&event(&state, TerminalEventKind::Started)));
        assert!(state.restart("/tmp".into()).is_err());
    }
}
