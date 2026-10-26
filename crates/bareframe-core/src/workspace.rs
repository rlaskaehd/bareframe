use crate::session::{SessionId, SessionState};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PaneId(pub u64);

/// Layout identity belongs to the workspace, not the terminal backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Terminal(SessionId),
}

#[derive(Debug)]
pub struct WorkspaceState {
    pub panes: BTreeMap<PaneId, PaneKind>,
    pub active_pane: PaneId,
}

impl WorkspaceState {
    pub fn single_terminal(session: &SessionState) -> Self {
        let pane = PaneId(1);
        Self {
            panes: BTreeMap::from([(pane, PaneKind::Terminal(session.key.id))]),
            active_pane: pane,
        }
    }
}
