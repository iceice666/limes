use std::sync::{Arc, Mutex};

use limes_proto::{AuthOutcome, AuthRequest, LimesEvent, LockState};

use limes_common::{AuthBackend, EventBus, LimesError, LockAuthBackend, Result};

pub trait DisplayBackend: Send + Sync {
    fn lock(&self) -> Result<()>;
    fn unlock(&self) -> Result<()>;
}

/// Placeholder display backend.
///
/// Replace this with Wayland/gtk-session-lock, VT switching, or another display
/// implementation. Keeping it behind a trait lets UI experiments continue while
/// the real lock compositor integration is developed.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopDisplayBackend;

impl DisplayBackend for NoopDisplayBackend {
    fn lock(&self) -> Result<()> {
        Err(LimesError::Lock(
            "no display/session-lock backend is configured".to_owned(),
        ))
    }

    fn unlock(&self) -> Result<()> {
        Err(LimesError::Lock(
            "no display/session-lock backend is configured".to_owned(),
        ))
    }
}

struct AuthBackendAdapter {
    auth: Arc<dyn AuthBackend>,
}

impl LockAuthBackend for AuthBackendAdapter {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        AuthBackend::authenticate(&*self.auth, request)
    }

    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()> {
        AuthBackend::close_session(&*self.auth, auth_session_id)
    }
}

pub struct LockManager {
    state: Mutex<LockState>,
    display: Arc<dyn DisplayBackend>,
    auth: Arc<dyn LockAuthBackend>,
    events: EventBus,
}

impl LockManager {
    /// Creates a lock manager from the full login auth trait.
    ///
    /// Prefer `with_lock_auth` for lock-only auth backends that cannot open
    /// login sessions.
    #[must_use]
    pub fn new(
        display: Arc<dyn DisplayBackend>,
        auth: Arc<dyn AuthBackend>,
        events: EventBus,
    ) -> Self {
        Self::with_lock_auth(display, Arc::new(AuthBackendAdapter { auth }), events)
    }

    /// Creates a lock manager from the narrow lock auth trait.
    #[must_use]
    pub fn with_lock_auth(
        display: Arc<dyn DisplayBackend>,
        auth: Arc<dyn LockAuthBackend>,
        events: EventBus,
    ) -> Self {
        Self {
            state: Mutex::new(LockState::Unlocked),
            display,
            auth,
            events,
        }
    }

    pub fn lock_now(&self) -> Result<()> {
        self.set_state(LockState::Locking)?;
        match self.display.lock() {
            Ok(()) => self.set_state(LockState::Locked),
            Err(error) => {
                self.set_state_lossy(LockState::Unlocked);
                Err(error)
            }
        }
    }

    pub fn unlock(&self, request: &AuthRequest) -> AuthOutcome {
        if let Err(error) = self.set_state(LockState::Unlocking) {
            self.set_state_lossy(LockState::Locked);
            return Err(limes_proto::AuthFailure::Internal(error.to_string()));
        }

        self.events.emit(LimesEvent::AuthStarted {
            username: request.username.clone(),
        });

        let outcome = self.auth.authenticate(request);
        match &outcome {
            Ok(success) => {
                self.events.emit(LimesEvent::AuthSucceeded {
                    username: success.username.clone(),
                    uid: success.uid,
                });
                let display_result = self.display.unlock();
                let _ = self.auth.close_session(success.auth_session_id.as_deref());
                if let Err(error) = display_result {
                    self.set_state_lossy(LockState::Locked);
                    return Err(limes_proto::AuthFailure::Internal(error.to_string()));
                }
                self.set_state_lossy(LockState::Unlocked);
            }
            Err(reason) => {
                self.events.emit(LimesEvent::AuthFailed {
                    username: request.username.clone(),
                    reason: reason.to_string(),
                });
                self.set_state_lossy(LockState::Locked);
            }
        }
        outcome
    }

    pub fn state(&self) -> Result<LockState> {
        self.state
            .lock()
            .map(|state| *state)
            .map_err(|_| LimesError::Lock("lock state mutex poisoned".to_owned()))
    }

    fn set_state(&self, state: LockState) -> Result<()> {
        let mut current = self
            .state
            .lock()
            .map_err(|_| LimesError::Lock("lock state mutex poisoned".to_owned()))?;
        *current = state;
        drop(current);
        self.events.emit(LimesEvent::LockStateChanged { state });
        Ok(())
    }

    fn set_state_lossy(&self, state: LockState) {
        let _ = self.set_state(state);
    }
}
