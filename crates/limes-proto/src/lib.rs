//! Shared data types used by limes frontends and backend code.
//!
//! Keep this crate lightweight. A native frontend can depend on `limes-login`
//! or `limes-lock` directly, while an out-of-process/webview frontend can use
//! these types as the stable vocabulary for a future IPC/FFI boundary.

use std::fmt;

use zeroize::Zeroize;

/// Credentials collected by a frontend and submitted to the backend.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthRequest {
    pub username: String,
    pub password: String,
    pub tty: Option<String>,
}

impl AuthRequest {
    #[must_use]
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            tty: None,
        }
    }

    pub fn clear_secret(&mut self) {
        self.password.zeroize();
        self.password.clear();
    }
}

impl Drop for AuthRequest {
    fn drop(&mut self) {
        self.clear_secret();
    }
}

impl fmt::Debug for AuthRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthRequest")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("tty", &self.tty)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_request_debug_redacts_password() {
        let request = AuthRequest::new("alice", "secret");

        let rendered = format!("{request:?}");

        assert!(rendered.contains("alice"));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("secret"));
    }

    #[test]
    fn clear_secret_removes_password_value() {
        let mut request = AuthRequest::new("alice", "secret");

        request.clear_secret();

        assert!(request.password.is_empty());
    }
}

/// Successful authentication result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSuccess {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub home: Option<String>,
    pub shell: Option<String>,
    /// Opaque backend session id used to pair PAM open/close calls.
    pub auth_session_id: Option<String>,
}

/// Authentication failure categories frontends can render differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFailure {
    InvalidCredentials,
    LockedOut,
    BackendUnavailable(String),
    Internal(String),
}

impl fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => f.write_str("invalid credentials"),
            Self::LockedOut => f.write_str("account locked out"),
            Self::BackendUnavailable(reason) => {
                write!(f, "authentication backend unavailable: {reason}")
            }
            Self::Internal(reason) => write!(f, "authentication error: {reason}"),
        }
    }
}

pub type AuthOutcome = Result<AuthSuccess, AuthFailure>;

/// PAM conversation message kinds reported by authentication backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PamMessageKind {
    PromptEchoOn,
    PromptEchoOff,
    TextInfo,
    Error,
}

/// Lock state as understood by UI frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    Locking,
    Locked,
    Unlocking,
}

impl fmt::Display for LockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlocked => f.write_str("unlocked"),
            Self::Locking => f.write_str("locking"),
            Self::Locked => f.write_str("locked"),
            Self::Unlocking => f.write_str("unlocking"),
        }
    }
}

/// Frontend-renderable session choice supplied by the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChoice {
    pub name: String,
    /// `None` means use the backend default from `Config::session_spec_for`.
    pub command: Option<Vec<String>>,
}

impl SessionChoice {
    #[must_use]
    pub fn default_session() -> Self {
        Self {
            name: "Default session".to_owned(),
            command: None,
        }
    }
}

impl fmt::Display for SessionChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Command and environment used to start a user session after login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_dir: Option<String>,
    pub auth_session_id: Option<String>,
}

impl SessionSpec {
    #[must_use]
    pub fn new(username: impl Into<String>, uid: u32, gid: u32, command: Vec<String>) -> Self {
        Self {
            username: username.into(),
            uid,
            gid,
            command,
            env: Vec::new(),
            working_dir: None,
            auth_session_id: None,
        }
    }
}

/// Serializable/minimal session handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHandle {
    pub pid: u32,
    pub username: String,
    pub command: Vec<String>,
    pub auth_session_id: Option<String>,
}

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
