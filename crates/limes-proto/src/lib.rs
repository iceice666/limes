//! Shared data types used by limes frontends and backend code.
//!
//! Keep this crate lightweight. A native frontend can depend on `limes-login`
//! or `limes-lock` directly, while an out-of-process/webview frontend can use
//! these types as the stable vocabulary for a future IPC/FFI boundary.

pub mod auth;
pub mod events;
pub mod lock;
pub mod session;

pub use auth::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess, PamMessageKind};
pub use events::LimesEvent;
pub use lock::LockState;
pub use session::{SessionChoice, SessionHandle, SessionSpec};
