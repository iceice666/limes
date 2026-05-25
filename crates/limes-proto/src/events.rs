use crate::{LockState, PamMessageKind};

/// Events emitted by backend orchestration for logging/diagnostics/UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimesEvent {
    LockStateChanged {
        state: LockState,
    },
    AuthStarted {
        username: String,
    },
    AuthSucceeded {
        username: String,
        uid: u32,
    },
    AuthFailed {
        username: String,
        reason: String,
    },
    AuthPamMessage {
        username: String,
        kind: PamMessageKind,
        message: String,
    },
    SessionStarted {
        username: String,
        pid: u32,
    },
    FrontendStarted {
        mode: String,
        command: Vec<String>,
    },
}
