use limes_proto::SessionChoice;

use crate::session_catalog::command::split_words;

pub(super) fn parse_session_entry(value: &str) -> Option<SessionChoice> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_session_entry_splits_simple_env_commands() {
        let choice = parse_session_entry("Test=/usr/bin/start --flag").unwrap();

        assert_eq!(choice.name, "Test");
        assert_eq!(
            choice.command,
            Some(vec!["/usr/bin/start".to_owned(), "--flag".to_owned()])
        );
    }

    #[test]
    fn parse_session_entry_rejects_missing_command() {
        assert!(parse_session_entry("Test=").is_none());
    }
}
