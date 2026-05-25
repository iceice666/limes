//! Screenlock library for limes frontends.
//!
//! This crate owns lock state, unlock authentication orchestration, and display
//! session-lock backends. It intentionally does not contain login session launch
//! logic.

pub mod display;
pub mod manager;
pub mod runtime;

pub use limes_common as common;
pub use limes_proto as proto;

pub use common::{
    Config, EventBus, EventSink, FrontendMode, FrontendRunner, FrontendSpec, LimesError,
    LockAuthBackend, NoopLockBackend, PAM_SERVICE, PamAuth, Result, StderrEventSink,
};
pub use display::{DisplayBackend, NoopDisplayBackend, WaylandSessionLockBackend};
pub use manager::LockManager;
pub use proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess, LimesEvent, LockState};
pub use runtime::LockRuntime;
