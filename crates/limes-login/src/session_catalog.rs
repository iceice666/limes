use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use limes_proto::SessionChoice;

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

fn session_desktop_dirs() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(home));
    } else if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share"));
    }

    if let Some(value) = env::var_os("XDG_DATA_DIRS") {
        roots.extend(env::split_paths(&value));
    } else {
        roots.extend([
            PathBuf::from("/run/current-system/sw/share"),
            PathBuf::from("/etc/profiles/per-user")
                .join(env::var("USER").unwrap_or_default())
                .join("share"),
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]);
    }

    let mut dirs = Vec::new();
    for root in roots {
        for subdir in ["wayland-sessions", "xsessions"] {
            let directory = root.join(subdir);
            if !dirs.iter().any(|existing| existing == &directory) {
                dirs.push(directory);
            }
        }
    }

    dirs
}

fn parse_desktop_session(path: &Path) -> Option<SessionChoice> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_desktop_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut try_exec = None;
    let mut hidden = false;
    let mut no_display = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();

        match key.trim() {
            "Name" => name = Some(value.to_owned()),
            "Exec" => exec = Some(value.to_owned()),
            "TryExec" => try_exec = Some(value.to_owned()),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if hidden || no_display {
        return None;
    }

    if let Some(try_exec) = try_exec.as_deref() {
        if !command_available(try_exec) {
            return None;
        }
    }

    let command = parse_desktop_exec(exec.as_deref()?)?;
    if !command
        .first()
        .is_some_and(|program| command_available(program))
    {
        return None;
    }

    let name = name.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Session")
            .to_owned()
    });

    Some(SessionChoice {
        name,
        command: Some(command),
    })
}

fn parse_session_entry(value: &str) -> Option<SessionChoice> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let (name, command) = value.split_once('=')?;
    let command = split_words(command);
    if name.trim().is_empty() || command.is_empty() {
        return None;
    }

    Some(SessionChoice {
        name: name.trim().to_owned(),
        command: Some(command),
    })
}

fn parse_desktop_exec(exec: &str) -> Option<Vec<String>> {
    let command = split_desktop_words(exec)
        .into_iter()
        .filter_map(|word| clean_desktop_exec_word(&word))
        .collect::<Vec<_>>();

    (!command.is_empty()).then_some(command)
}

fn split_words(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn split_desktop_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn clean_desktop_exec_word(word: &str) -> Option<String> {
    if matches!(
        word,
        "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%i" | "%c" | "%k"
    ) {
        return None;
    }

    let mut cleaned = String::new();
    let mut chars = word.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('%') => cleaned.push('%'),
                Some(_) => {}
                None => cleaned.push('%'),
            }
        } else {
            cleaned.push(ch);
        }
    }

    (!cleaned.is_empty()).then_some(cleaned)
}

fn push_session(sessions: &mut Vec<SessionChoice>, entry: SessionChoice) {
    if !sessions.iter().any(|existing| existing.name == entry.name) {
        sessions.push(entry);
    }
}

fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists();
    }

    command_in_path(command)
}

fn command_in_path(command: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| path.join(command).is_file())
}
