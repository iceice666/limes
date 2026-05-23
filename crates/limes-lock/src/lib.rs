//! Screenlock library for limes frontends.
//!
//! This crate owns lock state, unlock authentication orchestration, and display
//! session-lock backends. It intentionally does not contain login session launch
//! logic.

pub mod lock;
pub mod runtime;
pub mod wayland_lock;

pub use limes_common as common;
pub use limes_proto as proto;

pub use common::{
    Config, EventBus, EventSink, FrontendMode, FrontendRunner, FrontendSpec, LimesError,
    LockAuthBackend, PAM_SERVICE, PamAuth, Result, StderrEventSink,
};
pub use lock::{DisplayBackend, LockManager, NoopDisplayBackend};
pub use proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess, LimesEvent, LockState};
pub use runtime::LockRuntime;
pub use wayland_lock::WaylandSessionLockBackend;
