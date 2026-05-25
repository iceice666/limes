mod command;
mod desktop;
mod dirs;
mod env_entries;

use std::env;
use std::fs;

use limes_proto::SessionChoice;

use crate::session_catalog::desktop::parse_desktop_session;
use crate::session_catalog::dirs::session_desktop_dirs;
use crate::session_catalog::env_entries::parse_session_entry;

/// Discovers login session choices from the system session desktop files.
#[must_use]
pub fn discover_available_sessions() -> Vec<SessionChoice> {
    let mut sessions = Vec::new();

    for directory in session_desktop_dirs() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "desktop")
            {
                if let Some(session) = parse_desktop_session(&path) {
                    push_session(&mut sessions, session);
                }
            }
        }
    }

    // Manual backend-provided extras for systems without .desktop session files.
    // Format: Name=command args;Other=session-command
    for env_name in ["LIMES_SESSIONS", "LIMES_ICED_SESSIONS"] {
        if let Ok(value) = env::var(env_name) {
            for entry in value.split(';').filter_map(parse_session_entry) {
                push_session(&mut sessions, entry);
            }
        }
    }

    if sessions.is_empty() {
        sessions.push(SessionChoice::default_session());
    }

    sessions
}

fn push_session(sessions: &mut Vec<SessionChoice>, entry: SessionChoice) {
    if !sessions.iter().any(|existing| existing.name == entry.name) {
        sessions.push(entry);
    }
}
