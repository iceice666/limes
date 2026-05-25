use limes_proto::{AuthOutcome, AuthRequest, AuthSuccess};

use crate::Result;

/// Full authentication backend used by login managers.
///
/// `open_session` opens the PAM/login session before launching the user's
/// desktop command, and `close_session` closes or cleans up the opaque
/// authentication transaction returned in `AuthSuccess::auth_session_id`.
pub trait AuthBackend: Send + Sync {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome;
    fn open_session(&self, user: &AuthSuccess) -> Result<Vec<(String, String)>>;
    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()>;
}

/// Narrow authentication backend used by lock flows.
///
/// Locking only needs to authenticate an unlock request and clean up the opaque
/// auth transaction after success; it must not open a login session.
pub trait LockAuthBackend: Send + Sync {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome;
    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()>;
}

impl<T: AuthBackend + ?Sized> LockAuthBackend for T {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        AuthBackend::authenticate(self, request)
    }

    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()> {
        AuthBackend::close_session(self, auth_session_id)
    }
}
