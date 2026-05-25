use std::fmt;

/// Frontend-renderable session choice supplied by the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChoice {
    pub name: String,
    /// `None` means use the backend default from `Config::session_spec_for`.
    pub command: Option<Vec<String>>,
}

impl SessionChoice {
    #[must_use]
    pub fn default_session() -> Self {
        Self {
            name: "Default session".to_owned(),
            command: None,
        }
    }
}

impl fmt::Display for SessionChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Command and environment used to start a user session after login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_dir: Option<String>,
    pub auth_session_id: Option<String>,
}

impl SessionSpec {
    #[must_use]
    pub fn new(username: impl Into<String>, uid: u32, gid: u32, command: Vec<String>) -> Self {
        Self {
            username: username.into(),
            uid,
            gid,
            command,
            env: Vec::new(),
            working_dir: None,
            auth_session_id: None,
        }
    }
}

/// Serializable/minimal session handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHandle {
    pub pid: u32,
    pub username: String,
    pub command: Vec<String>,
    pub auth_session_id: Option<String>,
}
