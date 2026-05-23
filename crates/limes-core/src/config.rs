use std::env;

use limes_proto::{AuthSuccess, SessionSpec};

use crate::error::{LimesError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthBackendKind {
    /// Production target backed by Linux-PAM.
    Pam,
    /// Explicit development backend. Enable with `LIMES_AUTH_BACKEND=dev` and
    /// set `LIMES_DEV_PASSWORD`.
    DevPassword {
        password: String,
    },
    DenyAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendSpec {
    /// Minimal text frontend built into `limes login`, useful for smoke tests.
    BuiltinText,
    /// Any executable. Webview/native frontends can be launched this way and may
    /// link to `limes-core` themselves.
    External { program: String, args: Vec<String> },
}

impl FrontendSpec {
    #[must_use]
    pub fn external(program: impl Into<String>, args: Vec<String>) -> Self {
        Self::External {
            program: program.into(),
            args,
        }
    }

    #[must_use]
    pub fn command_line(&self) -> Option<Vec<String>> {
        match self {
            Self::BuiltinText => None,
            Self::External { program, args } => {
                let mut command = Vec::with_capacity(args.len() + 1);
                command.push(program.clone());
                command.extend(args.iter().cloned());
                Some(command)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub auth_backend: AuthBackendKind,
    pub pam_service: String,
    pub login_frontend: FrontendSpec,
    pub lock_frontend: Option<FrontendSpec>,
    pub session_command: Vec<String>,
    pub max_auth_attempts: u8,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let auth_backend = match env::var("LIMES_AUTH_BACKEND").as_deref() {
            Ok("dev") => {
                let password = env::var("LIMES_DEV_PASSWORD").map_err(|_| {
                    LimesError::Config(
                        "LIMES_AUTH_BACKEND=dev requires LIMES_DEV_PASSWORD".to_owned(),
                    )
                })?;
                AuthBackendKind::DevPassword { password }
            }
            Ok("deny") => AuthBackendKind::DenyAll,
            Ok("pam") | Err(_) => AuthBackendKind::Pam,
            Ok(other) => {
                return Err(LimesError::Config(format!(
                    "unknown LIMES_AUTH_BACKEND `{other}`; expected pam, dev, or deny"
                )));
            }
        };

        let pam_service = env::var("LIMES_PAM_SERVICE").unwrap_or_else(|_| "limes".to_owned());
        let login_frontend = match env::var("LIMES_LOGIN_FRONTEND") {
            Ok(value) if value.trim().is_empty() => FrontendSpec::BuiltinText,
            Ok(value) if matches!(value.as_str(), "builtin" | "text") => FrontendSpec::BuiltinText,
            Ok(value) => parse_external_command(&value)?,
            Err(_) => FrontendSpec::BuiltinText,
        };
        let lock_frontend = match env::var("LIMES_LOCK_FRONTEND") {
            Ok(value) if value.trim().is_empty() => None,
            Ok(value) => Some(parse_external_command(&value)?),
            Err(_) => None,
        };
        let session_command = match env::var("LIMES_SESSION_COMMAND") {
            Ok(value) if !value.trim().is_empty() => split_words(&value),
            _ => Vec::new(),
        };
        let max_auth_attempts = env::var("LIMES_MAX_AUTH_ATTEMPTS")
            .ok()
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(3)
            .max(1);

        Ok(Self {
            auth_backend,
            pam_service,
            login_frontend,
            lock_frontend,
            session_command,
            max_auth_attempts,
        })
    }

    #[must_use]
    pub fn session_spec_for(&self, success: &AuthSuccess) -> SessionSpec {
        let command = if self.session_command.is_empty() {
            vec![success
                .shell
                .clone()
                .filter(|shell| !shell.trim().is_empty())
                .unwrap_or_else(|| "/bin/sh".to_owned())]
        } else {
            self.session_command.clone()
        };

        let mut spec =
            SessionSpec::new(success.username.clone(), success.uid, success.gid, command);
        spec.auth_session_id = success.auth_session_id.clone();
        spec.env.push(("USER".to_owned(), success.username.clone()));
        spec.env
            .push(("LOGNAME".to_owned(), success.username.clone()));
        spec.env.push(("UID".to_owned(), success.uid.to_string()));
        spec.env.push(("GID".to_owned(), success.gid.to_string()));
        if let Some(home) = &success.home {
            spec.env.push(("HOME".to_owned(), home.clone()));
            spec.env.push(("PWD".to_owned(), home.clone()));
            spec.working_dir = Some(home.clone());
        }
        if let Some(shell) = &success.shell {
            spec.env.push(("SHELL".to_owned(), shell.clone()));
        }
        spec.env.push((
            "PATH".to_owned(),
            env::var("LIMES_SESSION_PATH").unwrap_or_else(|_| {
                "/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin".to_owned()
            }),
        ));
        spec
    }
}

fn parse_external_command(value: &str) -> Result<FrontendSpec> {
    let words = split_words(value);
    let (program, args) = words
        .split_first()
        .ok_or_else(|| LimesError::Config("empty frontend command".to_owned()))?;
    Ok(FrontendSpec::external(program.clone(), args.to_vec()))
}

/// Tiny whitespace splitter for env-provided commands.
///
/// Prefer CLI `--frontend PROGRAM -- ARGS...` for anything that needs quoting.
fn split_words(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(std::borrow::ToOwned::to_owned)
        .collect()
}
