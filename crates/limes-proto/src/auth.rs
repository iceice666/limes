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
