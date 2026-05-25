use std::fmt;

/// Lock state as understood by UI frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    Locking,
    Locked,
    Unlocking,
}

impl fmt::Display for LockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlocked => f.write_str("unlocked"),
            Self::Locking => f.write_str("locking"),
            Self::Locked => f.write_str("locked"),
            Self::Unlocking => f.write_str("unlocking"),
        }
    }
}
