use std::sync::Arc;

use limes_proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess, LimesEvent, SessionHandle};

use crate::auth::{AuthBackend, DenyAllAuth, DevAuth, PamAuth};
use crate::config::{AuthBackendKind, Config, FrontendSpec};
use crate::error::{LimesError, Result};
use crate::events::{EventBus, StderrEventSink};
use crate::frontend::{FrontendMode, FrontendRunner};
use crate::lock::{LockManager, NoopDisplayBackend};
use crate::session::{LocalSessionBackend, SessionManager};

pub struct Runtime {
    config: Config,
    auth: Arc<dyn AuthBackend>,
    lock: LockManager,
    sessions: SessionManager,
    frontends: FrontendRunner,
    events: EventBus,
}

impl Runtime {
    pub fn from_env() -> Result<Self> {
        Self::from_config(Config::from_env()?)
    }

    pub fn from_config(config: Config) -> Result<Self> {
        let events = EventBus::new();
        if std::env::var_os("LIMES_LOG_EVENTS").is_some() {
            events.subscribe(Arc::new(StderrEventSink));
        }

        let auth: Arc<dyn AuthBackend> = match &config.auth_backend {
            AuthBackendKind::Pam => Arc::new(PamAuth::with_events(
                config.pam_service.clone(),
                Some(events.clone()),
            )),
            AuthBackendKind::DevPassword { password } => Arc::new(DevAuth::new(password.clone())),
            AuthBackendKind::DenyAll => Arc::new(DenyAllAuth),
        };

        let lock = LockManager::new(
            Arc::new(NoopDisplayBackend),
            Arc::clone(&auth),
            events.clone(),
        );
        let sessions = SessionManager::new(Arc::new(LocalSessionBackend), events.clone());
        let frontends = FrontendRunner::new(events.clone());

        Ok(Self {
            config,
            auth,
            lock,
            sessions,
            frontends,
            events,
        })
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub fn events(&self) -> EventBus {
        self.events.clone()
    }

    pub fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        self.events.emit(LimesEvent::AuthStarted {
            username: request.username.clone(),
        });

        let outcome = self.auth.authenticate(request);
        match &outcome {
            Ok(success) => self.events.emit(LimesEvent::AuthSucceeded {
                username: success.username.clone(),
                uid: success.uid,
            }),
            Err(reason) => self.events.emit(LimesEvent::AuthFailed {
                username: request.username.clone(),
                reason: reason.to_string(),
            }),
        }
        outcome
    }

    pub fn start_session_for(&self, success: &AuthSuccess) -> Result<SessionHandle> {
        let pam_env = self.auth.open_session(success)?;
        let mut spec = self.config.session_spec_for(success);
        spec.env.extend(pam_env);
        match self.sessions.start(&spec) {
            Ok(handle) => Ok(handle),
            Err(error) => {
                let _ = self.auth.close_session(success.auth_session_id.as_deref());
                Err(error)
            }
        }
    }

    pub fn wait_session(&self, handle: &SessionHandle) -> Result<i32> {
        let wait_result = self.sessions.wait(handle);
        let close_result = self.auth.close_session(handle.auth_session_id.as_deref());
        match (wait_result, close_result) {
            (Ok(status), Ok(())) => Ok(status),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    pub fn lock_now(&self) -> Result<()> {
        self.lock.lock_now()
    }

    pub fn unlock(&self, request: &AuthRequest) -> AuthOutcome {
        self.lock.unlock(request)
    }

    pub fn launch_login_frontend(&self) -> Result<i32> {
        self.launch_frontend(&self.config.login_frontend, FrontendMode::Login)
    }

    pub fn launch_lock_frontend(&self) -> Result<Option<i32>> {
        match &self.config.lock_frontend {
            Some(spec) => self.launch_frontend(spec, FrontendMode::Lock).map(Some),
            None => Ok(None),
        }
    }

    fn launch_frontend(&self, spec: &FrontendSpec, mode: FrontendMode) -> Result<i32> {
        let status = self.frontends.run(spec, mode)?;
        Ok(status.code().unwrap_or(1))
    }
}

impl From<AuthFailure> for LimesError {
    fn from(value: AuthFailure) -> Self {
        Self::Auth(value.to_string())
    }
}
