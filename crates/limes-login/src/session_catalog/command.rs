use std::env;
use std::path::Path;

pub(super) fn split_words(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) fn split_desktop_words(value: &str) -> Vec<String> {
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

pub(super) fn clean_desktop_exec_word(word: &str) -> Option<String> {
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

pub(super) fn command_available(command: &str) -> bool {
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
