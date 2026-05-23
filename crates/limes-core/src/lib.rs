//! Login manager and screenlock library for limes frontends.
//!
//! This crate owns security-sensitive logic: authentication, lock state,
//! PAM/session boundaries, and session launch. UI code should live in a frontend
//! executable and call this crate instead of duplicating auth/session logic.

pub mod auth;
pub mod config;
pub mod error;
pub mod events;
pub mod frontend;
pub mod lock;
pub mod runtime;
pub mod session;
pub mod session_catalog;
pub mod wayland_lock;

pub use auth::{AuthBackend, PAM_SERVICE, PamAuth};
pub use config::{Config, FrontendSpec};
pub use error::{LimesError, Result};
pub use events::{EventBus, EventSink, StderrEventSink};
pub use frontend::{FrontendMode, FrontendRunner};
pub use lock::{DisplayBackend, LockManager, NoopDisplayBackend};
pub use runtime::Runtime;
pub use session::{LocalSessionBackend, SessionBackend, SessionManager};
pub use wayland_lock::WaylandSessionLockBackend;
