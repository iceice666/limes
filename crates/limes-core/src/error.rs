use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimesError {
    Auth(String),
    Config(String),
    Frontend(String),
    Lock(String),
    Session(String),
    Unsupported(String),
    Io(String),
}

pub type Result<T> = std::result::Result<T, LimesError>;

impl fmt::Display for LimesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(message) => write!(f, "auth error: {message}"),
            Self::Config(message) => write!(f, "config error: {message}"),
            Self::Frontend(message) => write!(f, "frontend error: {message}"),
            Self::Lock(message) => write!(f, "lock error: {message}"),
            Self::Session(message) => write!(f, "session error: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
            Self::Io(message) => write!(f, "io error: {message}"),
        }
    }
}

impl std::error::Error for LimesError {}

impl From<std::io::Error> for LimesError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}
