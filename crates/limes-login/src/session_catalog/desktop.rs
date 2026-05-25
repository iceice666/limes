use std::fs;
use std::path::Path;

use limes_proto::SessionChoice;

use crate::session_catalog::command::{
    clean_desktop_exec_word, command_available, split_desktop_words,
};

pub(super) fn parse_desktop_session(path: &Path) -> Option<SessionChoice> {
    let content = fs::read_to_string(path).ok()?;
    parse_desktop_session_content(&content, path)
}

fn parse_desktop_session_content(content: &str, path: &Path) -> Option<SessionChoice> {
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

fn parse_desktop_exec(exec: &str) -> Option<Vec<String>> {
    let command = split_desktop_words(exec)
        .into_iter()
        .filter_map(|word| clean_desktop_exec_word(&word))
        .collect::<Vec<_>>();

    (!command.is_empty()).then_some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_desktop_exec_keeps_quoted_words_together() {
        let command = parse_desktop_exec(r#"env FOO="bar baz" start-session"#).unwrap();

        assert_eq!(
            command,
            vec![
                "env".to_owned(),
                "FOO=bar baz".to_owned(),
                "start-session".to_owned()
            ]
        );
    }

    #[test]
    fn parse_desktop_exec_removes_field_codes() {
        let command = parse_desktop_exec("start-session %f --literal %%").unwrap();

        assert_eq!(
            command,
            vec![
                "start-session".to_owned(),
                "--literal".to_owned(),
                "%".to_owned()
            ]
        );
    }
}
