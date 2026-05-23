use std::sync::Arc;

use limes_common::{
    AuthBackend, Config, EventBus, FrontendMode, FrontendRunner, FrontendSpec, LimesError, PamAuth,
    Result,
};
use limes_proto::{
    AuthOutcome, AuthRequest, AuthSuccess, LimesEvent, SessionChoice, SessionHandle,
};

use crate::session::{LocalSessionBackend, SessionBackend, SessionManager};
use crate::session_catalog;

pub struct LoginRuntime {
    config: Config,
    auth: Arc<dyn AuthBackend>,
    sessions: SessionManager,
    frontends: FrontendRunner,
    events: EventBus,
}

impl LoginRuntime {
    pub fn from_env() -> Result<Self> {
        Self::from_config(Config::from_env()?)
    }

    pub fn from_config(config: Config) -> Result<Self> {
        let events = EventBus::from_env();
        let auth: Arc<dyn AuthBackend> = Arc::new(PamAuth::with_events(Some(events.clone())));
        Ok(Self::with_parts(config, auth, events))
    }

    #[must_use]
    pub fn with_parts(config: Config, auth: Arc<dyn AuthBackend>, events: EventBus) -> Self {
        Self::with_session_backend(config, auth, Arc::new(LocalSessionBackend), events)
    }

    #[must_use]
    pub fn with_session_backend(
        config: Config,
        auth: Arc<dyn AuthBackend>,
        backend: Arc<dyn SessionBackend>,
        events: EventBus,
    ) -> Self {
        let sessions = SessionManager::new(backend, events.clone());
        let frontends = FrontendRunner::new(events.clone());

        Self {
            config,
            auth,
            sessions,
            frontends,
            events,
        }
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

    #[must_use]
    pub fn available_sessions(&self) -> Vec<SessionChoice> {
        session_catalog::discover_available_sessions()
    }

    pub fn start_session_for(&self, success: &AuthSuccess) -> Result<SessionHandle> {
        self.start_session_for_command_override(success, None)
    }

    /// Starts a session with a frontend-selected command while keeping PAM and
    /// user context switching inside the login crate.
    pub fn start_session_for_with_command(
        &self,
        success: &AuthSuccess,
        command: Vec<String>,
    ) -> Result<SessionHandle> {
        if command.is_empty() {
            return Err(LimesError::Session(
                "selected session command is empty".to_owned(),
            ));
        }

        self.start_session_for_command_override(success, Some(command))
    }

    fn start_session_for_command_override(
        &self,
        success: &AuthSuccess,
        command: Option<Vec<String>>,
    ) -> Result<SessionHandle> {
        let pam_env = self.auth.open_session(success)?;
        let mut spec = self.config.session_spec_for(success);
        if let Some(command) = command {
            spec.command = command;
        }
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

    pub fn launch_login_frontend(&self) -> Result<i32> {
        let spec = self.config.login_frontend.as_ref().ok_or_else(|| {
            LimesError::Config(
                "no login frontend configured; set LIMES_LOGIN_FRONTEND or Config::login_frontend"
                    .to_owned(),
            )
        })?;
        self.launch_frontend(spec, FrontendMode::Login)
    }

    fn launch_frontend(&self, spec: &FrontendSpec, mode: FrontendMode) -> Result<i32> {
        let status = self.frontends.run(spec, mode)?;
        Ok(status.code().unwrap_or(1))
    }
}
