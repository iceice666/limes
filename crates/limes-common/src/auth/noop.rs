use limes_proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess};

use crate::auth::traits::AuthBackend;
use crate::{LimesError, Result};

/// Authentication backend placeholder for tests and explicit no-backend setups.
///
/// This backend never authenticates users and cannot open login sessions. It is
/// useful when callers need to construct lock/login runtime pieces without
/// configuring PAM, but it must not be treated as a real security backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopLockBackend;

impl AuthBackend for NoopLockBackend {
    fn authenticate(&self, _request: &AuthRequest) -> AuthOutcome {
        Err(AuthFailure::BackendUnavailable(
            "no lock authentication backend is configured".to_owned(),
        ))
    }

    fn open_session(&self, user: &AuthSuccess) -> Result<Vec<(String, String)>> {
        Err(LimesError::Auth(format!(
            "noop lock backend cannot open a login session for {}",
            user.username
        )))
    }

    fn close_session(&self, _auth_session_id: Option<&str>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::traits::LockAuthBackend;

    #[test]
    fn noop_lock_backend_rejects_authentication() {
        let backend = NoopLockBackend;
        let request = AuthRequest::new("alice", "secret");

        let outcome = AuthBackend::authenticate(&backend, &request);

        assert_eq!(
            outcome,
            Err(AuthFailure::BackendUnavailable(
                "no lock authentication backend is configured".to_owned()
            ))
        );
    }

    #[test]
    fn noop_lock_backend_is_usable_as_lock_auth_backend() {
        let backend: &dyn LockAuthBackend = &NoopLockBackend;
        let request = AuthRequest::new("alice", "secret");

        assert!(matches!(
            backend.authenticate(&request),
            Err(AuthFailure::BackendUnavailable(_))
        ));
        assert!(backend.close_session(Some("unused")).is_ok());
    }

    #[test]
    fn noop_lock_backend_cannot_open_login_sessions() {
        let backend = NoopLockBackend;
        let success = AuthSuccess {
            username: "alice".to_owned(),
            uid: 1000,
            gid: 1000,
            home: Some("/home/alice".to_owned()),
            shell: Some("/bin/sh".to_owned()),
            auth_session_id: None,
        };

        let error = backend.open_session(&success).unwrap_err();

        assert!(matches!(error, LimesError::Auth(_)));
    }
}
