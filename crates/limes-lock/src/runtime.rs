use std::sync::Arc;

use limes_common::{
    Config, EventBus, FrontendMode, FrontendRunner, FrontendSpec, LimesError, LockAuthBackend,
    PamLockAuth, Result,
};
use limes_proto::{AuthOutcome, AuthRequest};

use crate::lock::{DisplayBackend, LockManager};
use crate::wayland_lock::WaylandSessionLockBackend;

pub struct LockRuntime {
    config: Config,
    lock: LockManager,
    frontends: FrontendRunner,
    events: EventBus,
}

impl LockRuntime {
    pub fn from_env() -> Result<Self> {
        Self::from_config(Config::from_env()?)
    }

    pub fn from_config(config: Config) -> Result<Self> {
        let events = EventBus::from_env();
        let auth: Arc<dyn LockAuthBackend> =
            Arc::new(PamLockAuth::with_events(Some(events.clone())));
        let display: Arc<dyn DisplayBackend> = Arc::new(WaylandSessionLockBackend::default());
        Ok(Self::with_parts(config, auth, display, events))
    }

    #[must_use]
    pub fn with_parts(
        config: Config,
        auth: Arc<dyn LockAuthBackend>,
        display: Arc<dyn DisplayBackend>,
        events: EventBus,
    ) -> Self {
        let lock = LockManager::with_lock_auth(display, auth, events.clone());
        let frontends = FrontendRunner::new(events.clone());

        Self {
            config,
            lock,
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

    pub fn lock_now(&self) -> Result<()> {
        self.lock.lock_now()
    }

    pub fn unlock(&self, request: &AuthRequest) -> AuthOutcome {
        self.lock.unlock(request)
    }

    /// Authenticates an unlock request without releasing the display backend.
    ///
    /// Use this when the frontend owns the compositor session-lock surface and
    /// will issue the compositor unlock after successful authentication.
    pub fn authenticate_unlock(&self, request: &AuthRequest) -> AuthOutcome {
        self.lock.authenticate_unlock(request)
    }

    pub fn launch_lock_frontend(&self) -> Result<i32> {
        let spec = self.config.lock_frontend.as_ref().ok_or_else(|| {
            LimesError::Config(
                "no lock frontend configured; set LIMES_LOCK_FRONTEND or Config::lock_frontend"
                    .to_owned(),
            )
        })?;
        self.launch_frontend(spec, FrontendMode::Lock)
    }

    fn launch_frontend(&self, spec: &FrontendSpec, mode: FrontendMode) -> Result<i32> {
        let status = self.frontends.run(spec, mode)?;
        Ok(status.code().unwrap_or(1))
    }
}
