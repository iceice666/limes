//! Login-manager and session-launch library for limes frontends.
//!
//! This crate handles login authentication orchestration, PAM session
//! open/close boundaries, user session launching, and session discovery. It
//! intentionally does not contain screenlock/display-lock logic.

pub mod runtime;
pub mod session;
pub mod session_catalog;

pub use limes_common::{
    AuthBackend, Config, EventBus, EventSink, FrontendMode, FrontendRunner, FrontendSpec,
    LimesError, PAM_SERVICE, PamAuth, Result, StderrEventSink,
};
pub use runtime::LoginRuntime;
pub use session::{LocalSessionBackend, SessionBackend, SessionManager};
