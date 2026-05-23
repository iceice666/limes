//! Shared building blocks for limes login and screenlock crates.
//!
//! This crate owns code that both login-manager and screenlock flows need:
//! PAM authentication, common configuration, events, frontend launching, and
//! shared error handling.

pub mod auth;
pub mod config;
pub mod error;
pub mod events;
pub mod frontend;

pub use auth::{AuthBackend, LockAuthBackend, PAM_SERVICE, PamAuth};
pub use config::{Config, FrontendSpec};
pub use error::{LimesError, Result};
pub use events::{EventBus, EventSink, StderrEventSink};
pub use frontend::{FrontendMode, FrontendRunner};
