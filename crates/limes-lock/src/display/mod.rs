pub mod wayland;

use limes_common::{LimesError, Result};

pub use wayland::WaylandSessionLockBackend;

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
