use std::env;
use std::path::PathBuf;

pub(super) fn session_desktop_dirs() -> Vec<PathBuf> {
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
